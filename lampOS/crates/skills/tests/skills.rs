//! Every skill folder in `lampOS/skills/` against its own recordings, the
//! loader's refusals, and the program on a real bus with the body's and the
//! actions part's sockets stood in for.
use lamp_bus::Bus;
use lamp_msgs::{
    BodyGesture, Faces, GestureState, Hand, Light, LightPattern, LightRequest, PatternKind,
    Priority, SkillEvent, SkillStage, Speech, VoiceState,
};
use lamp_skills::{
    Admission, BODY_STEP_US, CLIP_GAP_US, Call, Engine, Permissions, SkillKind, clip_us, load,
    load_all, read_recordings, run_tests, tools_file,
};
use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixDatagram, UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// How often the program looks at its folder for a change.
const RELOAD: Duration = Duration::from_secs(2);

/// The program's clock as a test drives it (`--clock-control`,
/// `docs/skills.md`): the host's, and what this test skipped of it. The
/// program's own waits are on it: a skill's `for`, a clip's length, a pose
/// said again, the Policy's 2.5 s, the reload. A skip stands in for a wait
/// the test would sit through; anything after it runs at the host's pace.
/// [`Clock::host`] is the lamp's own instead: no flag, and each skip a real
/// wait, so a few tests still run the waits a lamp runs.
#[derive(Clone)]
struct Clock {
    /// The control socket; none on the host's clock.
    socket: Option<PathBuf>,
    skipped_us: Arc<AtomicU64>,
}

impl Clock {
    /// The clock of a program whose control socket is in `root`.
    fn at(root: &Path) -> Self {
        Self {
            socket: Some(root.join("clock.sock")),
            skipped_us: Arc::default(),
        }
    }

    /// The host's clock, as on a lamp: the program is given no flag.
    fn host() -> Self {
        Self {
            socket: None,
            skipped_us: Arc::default(),
        }
    }

    /// The flag that hands it to the program, if any.
    fn args(&self) -> Vec<&OsStr> {
        self.socket
            .iter()
            .flat_map(|socket| ["--clock-control".as_ref(), socket.as_os_str()])
            .collect()
    }

    /// Now, on the program's clock.
    fn now(&self) -> Instant {
        Instant::now() + Duration::from_micros(self.skipped_us.load(Ordering::SeqCst))
    }

    /// Skips the program's clock `by`, once it has taken in what was
    /// published before.
    fn skip(&self, by: Duration) {
        self.skip_after(by, || {});
    }

    /// As [`Clock::skip`], with `between` run once a stand-in's time has
    /// skipped and before the program's has.
    fn skip_after(&self, by: Duration, between: impl FnOnce()) {
        let Some(socket) = &self.socket else {
            std::thread::sleep(by);
            return between();
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(stream) => break stream,
                Err(error) => {
                    assert!(Instant::now() < deadline, "no clock control: {error}");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        let us = u64::try_from(by.as_micros()).unwrap();
        // Ahead here first: a stand-in that reads it is never behind what
        // the program does once it has skipped.
        self.skipped_us.fetch_add(us, Ordering::SeqCst);
        between();
        writeln!(&stream, r#"{{"skip_us":{us}}}"#).unwrap();
        // A program stuck before its loop fails the test, with why.
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        assert!(line.contains("now_us"), "{line}");
    }

    /// Skips the program's clock to `at`, if it is not there yet.
    fn skip_to(&self, at: Instant) {
        self.skip(at.saturating_duration_since(self.now()));
    }

    /// Waits for the program to be up and to have looked at its folder once:
    /// a skip of nothing, which its loop answers; on the host's clock, the
    /// 300 ms it takes.
    fn started(&self) {
        self.skip(if self.socket.is_some() {
            Duration::ZERO
        } else {
            Duration::from_millis(300)
        });
    }
}

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of Welcome back in `dir`, with one line of its `skill.toml` changed.
fn welcome_back_with(dir: &Path, from: &str, to: &str) -> PathBuf {
    let skill = dir.join("welcome-back");
    copy(&skills_root().join("welcome-back"), &skill);
    let toml = fs::read_to_string(skill.join("skill.toml")).unwrap();
    assert!(toml.contains(from), "{from}");
    fs::write(skill.join("skill.toml"), toml.replace(from, to)).unwrap();
    skill
}

#[test]
fn every_skill_folder_passes_its_own_recorded_tests() {
    let mut skills = 0;
    for entry in fs::read_dir(skills_root()).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        // Every folder loads; a case with no recording yet is not run.
        load(&entry.path()).unwrap_or_else(|why| panic!("{}: {why}", entry.path().display()));
        let (passed, _unrecorded) = run_tests(&entry.path())
            .unwrap_or_else(|why| panic!("{}: {why}", entry.path().display()));
        skills += passed;
    }
    assert!(skills >= 1);
}

#[test]
fn a_recorded_case_fails_when_the_skill_no_longer_does_what_it_expects() {
    let dir = tempfile::tempdir().unwrap();
    // Without the cooldown the lamp greets again at 1,400 s, which the
    // recording says it must not.
    let skill = welcome_back_with(dir.path(), "cooldown = \"20m\"", "cooldown = \"0s\"");
    let why = run_tests(&skill).unwrap_err();
    assert!(why.contains("back-after-ten-minutes"), "{why}");
    // A red light where warm was recorded.
    let skill = welcome_back_with(&dir.path().join("red"), "needs", "needs");
    fs::write(skill.join("lights/warm_glow.toml"), "colour = \"red\"\n").unwrap();
    let why = run_tests(&skill).unwrap_err();
    assert!(why.contains("call 2 is not \"light\""), "{why}");
}

#[test]
fn without_the_skip_nothing_the_skill_starts_is_done() {
    let skill = load(&skills_root().join("welcome-back")).unwrap();
    let mut engine = Engine::new(vec![skill], false);
    let case = skills_root().join("welcome-back/tests/back-after-ten-minutes");
    let mut runs = Vec::new();
    for message in read_recordings(&case).unwrap() {
        runs.extend(engine.observe(
            &message.header.topic,
            message.header.at_us,
            &message.payload,
            message.header.expires_ms,
        ));
    }
    // Each arrival after five minutes away, refused. A refusal does not
    // start the cooldown, so the one at 1,400 s is asked too.
    assert_eq!(runs.len(), 3);
    for run in runs {
        assert!(run.calls.is_empty());
        let stages: Vec<_> = run.events.iter().map(|event| event.stage).collect();
        assert_eq!(
            stages,
            [
                SkillStage::Triggered,
                SkillStage::Asked,
                SkillStage::Answered
            ]
        );
        assert_eq!(run.events[2].allowed, Some(false));
        run.events.iter().for_each(|event| event.check().unwrap());
    }
}

#[test]
fn the_loader_refuses_what_it_cannot_do() {
    let dir = tempfile::tempdir().unwrap();
    for (from, to, why) in [
        ("when = \"appears\"", "when = \"stays\"", "only \"appears\""),
        (
            "topic = \"humans/faces/tracked\"",
            "topic = \"air/state\"",
            "a field and one of",
        ),
        (
            "absent_for = \"5m\"",
            "absent_for = \"five minutes\"",
            "not a duration",
        ),
        (
            "cooldown = \"20m\"",
            "cooldown = \"20m\"\nsound = \"chime\"",
            "unknown field",
        ),
        ("look_at = \"face\"", "gesture = \"dance\"", "not one of"),
        ("[ask]", "[later]", "unknown field"),
        (
            "[ask]",
            "[[do]]\nsay = \"And again.\"\n[ask]",
            "one say or sound a skill",
        ),
        // Words the voice keeps (#939): only what it keeps, only `from`,
        // and still one say a skill.
        (
            "say = \"They are back at their desk. Greet them in one short line, by name if you know it.\"",
            "say = { from = \"calendar\" }",
            "say.from is \"schedule\" or \"tasks\", not \"calendar\"",
        ),
        (
            "say = \"They are back at their desk. Greet them in one short line, by name if you know it.\"",
            "say = { from = \"schedule\", note = \"x\" }",
            "a say table has from and nothing else",
        ),
        // A say that is neither, told in plain words (Fourth on #1033).
        (
            "say = \"They are back at their desk. Greet them in one short line, by name if you know it.\"",
            "say = 5",
            "say is a note in words, or { from = \"schedule\" } or { from = \"tasks\" }",
        ),
        (
            "[ask]",
            "[[do]]\nsay = { from = \"tasks\" }\n[ask]",
            "one say or sound a skill",
        ),
    ] {
        let skill = welcome_back_with(&dir.path().join(why.replace(' ', "_")), from, to);
        let error = load(&skill).unwrap_err();
        assert!(error.contains(why), "{to}: {error}");
    }
    let skill = welcome_back_with(&dir.path().join("light"), "needs", "needs");
    fs::write(skill.join("lights/warm_glow.toml"), "colour = \"gold\"\n").unwrap();
    assert!(load(&skill).unwrap_err().contains("not one of"));
    let skill = welcome_back_with(&dir.path().join("run"), "needs", "needs");
    fs::write(skill.join("run.py"), "").unwrap();
    assert!(load(&skill).unwrap_err().contains("run.py"));
    let skill = dir.path().join("renamed/hello");
    copy(&skills_root().join("welcome-back"), &skill);
    assert!(load(&skill).unwrap_err().contains("folder"));
}

/// A copy of the repository's skill `name` in `dir`, with `from` in its
/// `skill.toml` changed to `to`.
fn repo_skill_with(dir: &Path, name: &str, from: &str, to: &str) -> PathBuf {
    let skill = dir.join(name);
    copy(&skills_root().join(name), &skill);
    let toml = fs::read_to_string(skill.join("skill.toml")).unwrap();
    assert!(toml.contains(from), "{name}: {from}");
    fs::write(skill.join("skill.toml"), toml.replacen(from, to, 1)).unwrap();
    skill
}

/// The owner, 19 Oct: core skills are always on the lamp, as a phone's own
/// apps; the rest are installed. Desk tutor and its marks wait for the full
/// look in the product. Every one says its version.
#[test]
fn every_skill_folder_says_its_kind_and_version() {
    let (skills, errors) = load_all(&skills_root());
    assert!(errors.is_empty(), "{errors:?}");
    let named = |kind| -> Vec<&str> {
        skills
            .iter()
            .filter(|skill| skill.kind == kind)
            .map(|skill| skill.name.as_str())
            .collect()
    };
    assert_eq!(
        named(SkillKind::Core),
        [
            "dance",
            "laugh-glance",
            "light-out",
            "light-scenes",
            "light-show",
            "not-feeling-well",
            "point-light",
            "reminders",
            "stretch-break",
            "stuffy-air",
            "task-news",
            "welcome-back"
        ]
    );
    assert_eq!(
        named(SkillKind::Installable),
        ["desk-tutor", "desk-tutor-marks"]
    );
    for skill in &skills {
        assert_eq!(skill.version.as_deref(), Some("1.0.0"), "{}", skill.name);
    }
}

/// `kind` is core or installable, installable when it says none; a version
/// is three whole numbers.
#[test]
fn a_skills_kind_and_version_are_read_and_checked() {
    let dir = tempfile::tempdir().unwrap();
    let with = |name: &str, line: &str| {
        skill_with(
            dir.path(),
            name,
            &format!("{line}\n{}", AT_ONCE.trim_start()),
        )
    };
    assert_eq!(
        load(&with("plain", "")).unwrap().kind,
        SkillKind::Installable
    );
    assert_eq!(
        load(&with("own", "kind = \"core\"")).unwrap().kind,
        SkillKind::Core
    );
    assert_eq!(
        load(&with(
            "theirs",
            "kind = \"installable\"\nversion = \"2.13.0\""
        ))
        .unwrap()
        .version
        .as_deref(),
        Some("2.13.0")
    );
    let error = load(&with("app", "kind = \"app\"")).unwrap_err();
    assert!(error.contains("\"core\" or \"installable\""), "{error}");
    for version in ["1.0", "1.0.0.0", "01.0.0", "1.0.0-beta", "v1.0.0", ""] {
        let error = load(&with("versioned", &format!("version = {version:?}"))).unwrap_err();
        assert!(error.contains("MAJOR.MINOR.PATCH"), "{version}: {error}");
    }
}

/// Least privilege: a skill reads and does only what its `[permissions]`
/// lists; one that reads or does more is not loaded, and why says what it
/// lacks.
#[test]
fn a_skill_reads_and_does_only_what_its_permissions_declare() {
    let dir = tempfile::tempdir().unwrap();
    for (skill, from, to, why) in [
        (
            "welcome-back",
            "topics = [\"humans/faces/tracked\", \"voice/state\"]",
            "topics = [\"humans/faces/tracked\"]",
            "not permitted: a [[while]] reads voice/state, and voice/state is not in [permissions] topics",
        ),
        (
            "welcome-back",
            "topics = [\"humans/faces/tracked\", \"voice/state\"]",
            "topics = [\"voice/state\"]",
            "not permitted: a trigger reads humans/faces/tracked",
        ),
        (
            "welcome-back",
            "speak = true\n",
            "",
            "not permitted: say needs speak = true in [permissions]",
        ),
        (
            "welcome-back",
            "light = true\n",
            "",
            "not permitted: light needs light = true in [permissions]",
        ),
        (
            "welcome-back",
            "body = true\n",
            "",
            "not permitted: body.gaze needs body = true in [permissions]",
        ),
        // A look at a face reads the faces, whatever starts the skill.
        (
            "reminders",
            "topics = [\"schedule/due\", \"humans/faces/tracked\", \"voice/state\"]",
            "topics = [\"schedule/due\", \"voice/state\"]",
            "not permitted: a look at a face reads humans/faces/tracked",
        ),
        // What was heard is a person's words: [again] needs them declared.
        (
            "dance",
            "topics = [\"voice/heard\"]",
            "topics = []",
            "not permitted: [again] reads voice/heard",
        ),
        (
            "dance",
            "body = true\n",
            "",
            "not permitted: body.gesture needs body = true",
        ),
        (
            "stuffy-air",
            "light = true\n",
            "",
            "not permitted: light needs light = true",
        ),
        (
            "task-news",
            "speak = true\n",
            "",
            "not permitted: say needs speak = true",
        ),
        // The camera's picture for the model.
        (
            "desk-tutor",
            "look = true\n",
            "",
            "not permitted: look needs look = true",
        ),
        (
            "desk-tutor",
            "look = true\n",
            "look = true\ncamera = true\n",
            "unknown field `camera`",
        ),
        (
            "welcome-back",
            "\"voice/state\"]",
            "\"voice/state\", \"Voice/State\"]",
            "\"Voice/State\" is not a topic",
        ),
        (
            "welcome-back",
            "\"voice/state\"]",
            "\"voice/state\", \"voice/state\"]",
            "[permissions] topics lists one twice",
        ),
        (
            "welcome-back",
            "speak = true\n",
            "speak = true\nactions = [\"Volume-Up\"]\n",
            "\"Volume-Up\" is not a function",
        ),
    ] {
        let folder = dir
            .path()
            .join(why.replace([' ', '/', '[', ']', '"', '`'], "_"));
        let copied = repo_skill_with(&folder, skill, from, to);
        let error = load(&copied).unwrap_err();
        assert!(error.contains(why), "{skill}: {to}: {error}");
    }
    // No table at all: it may do nothing, so a skill that does anything is
    // refused, its first read first.
    let skill = skill_with(dir.path(), "none", "[permissions]\n");
    fs::write(skill.join("skill.toml"), AT_ONCE).unwrap();
    let error = load(&skill).unwrap_err();
    assert!(error.contains("not permitted: a trigger reads"), "{error}");
    // Declaring more than it reads is allowed; the program subscribes only
    // to what it reads.
    let skill = repo_skill_with(
        &dir.path().join("more"),
        "welcome-back",
        "\"voice/state\"]",
        "\"voice/state\", \"air/state\"]",
    );
    let engine = Engine::new(vec![load(&skill).unwrap()], true);
    assert_eq!(engine.topics(), ["humans/faces/tracked", "voice/state"]);
}

/// A skill that has the voice say the person's reminder or the news of
/// their work reads those words, so it declares their topic: a skill
/// started by the air could otherwise say the person's reminders at its own
/// moment (the reviewer on #1698, 08:39). It reads them through the voice,
/// so the program does not listen to the topic for it.
#[test]
fn a_say_of_the_persons_reminder_or_task_news_reads_its_topic() {
    let dir = tempfile::tempdir().unwrap();
    let toml = |topics: &str, from: &str| {
        format!(
            "[permissions]\ntopics = [{topics}]\nspeak = true\n\
             [trigger]\ntopic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\n\
             [ask]\npolicy = \"reminder\"\n\
             [[do]]\nsay = {{ from = \"{from}\" }}\n"
        )
    };
    for (from, topic) in [("schedule", "schedule/due"), ("tasks", "tasks/status")] {
        let skill = skill_with(
            &dir.path().join(from),
            "stuffy-says",
            &toml("\"air/state\"", from),
        );
        let error = load(&skill).unwrap_err();
        assert_eq!(
            error,
            format!(
                "not permitted: a say from {from} reads {topic}, and {topic} is not in [permissions] topics"
            )
        );
        let skill = skill_with(
            &dir.path().join(format!("{from}-declared")),
            "stuffy-says",
            &toml(&format!("\"air/state\", \"{topic}\""), from),
        );
        let engine = Engine::new(vec![load(&skill).unwrap()], true);
        assert_eq!(engine.topics(), ["air/state"]);
    }
    // Reminders and Task news say those words, and declare them.
    for name in ["reminders", "task-news"] {
        load(&skills_root().join(name)).unwrap();
    }
}

/// A function the model calls is one skill's. The reviewer's probe on
/// #1698 (08:39): an installable `a-dance` with `tool = "dance"`, first by
/// folder, took core Dance's function from the model without a word. Core
/// skills take their tools' names first, then the installable ones by
/// folder, and a skill whose tool's name is taken is not loaded, with why.
#[test]
fn a_tool_is_one_skills_a_core_skills_first() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    copy(&skills_root().join("dance"), &skills.join("dance"));
    let installable = |name: &str, tool: &str| {
        skill_with(
            &skills,
            name,
            &format!(
                "tool = \"{tool}\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"Say you will.\"\n"
            ),
        )
    };
    installable("a-dance", "dance");
    installable("b-wiggle", "wiggle");
    installable("c-wiggle", "wiggle");
    let (loaded, errors) = load_all(&skills);
    let names: Vec<&str> = loaded.iter().map(|skill| skill.name.as_str()).collect();
    assert_eq!(names, ["b-wiggle", "dance"]);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(
        errors[0].ends_with(
            "a-dance: its tool \"dance\" is the core skill dance's: a function the model calls is one skill's, \
             a core skill's first"
        ),
        "{errors:?}"
    );
    assert!(
        errors[1].ends_with("c-wiggle: its tool \"wiggle\" is the installable skill b-wiggle's: a function the model calls is one skill's, a core skill's first"),
        "{errors:?}"
    );
    let tools = tools_file(&loaded);
    let dance: Vec<&serde_json::Value> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tool| tool["name"] == "dance")
        .collect();
    assert_eq!(dance.len(), 1);
    assert!(
        dance[0]["description"]
            .as_str()
            .unwrap()
            .starts_with("When someone asks the lamp to dance"),
        "{tools}"
    );
    let mut engine = Engine::new(loaded, true);
    assert_eq!(
        engine
            .tool("dance", &serde_json::Value::Null, 0)
            .unwrap()
            .skill,
        "dance"
    );
    // The list says so too, as the program loads them.
    let listed = lamp_skills::package::list(&skills).unwrap();
    let line = listed
        .lines()
        .find(|line| line.starts_with("a-dance "))
        .unwrap();
    assert!(
        line.contains("not loaded: its tool \"dance\" is the core skill dance's"),
        "{listed}"
    );
}

/// The gate itself, as the program asks it where each call leaves: the
/// body, the light, the voice and the look each need their permission, and
/// the actions part's functions are the light's with `light`, any other only
/// by name.
#[test]
fn the_permissions_refuse_every_call_and_action_they_do_not_give() {
    let none = Permissions::default();
    let calls = [
        (Call::Gaze { face: None }, "body"),
        (Call::Gesture("nod".into()), "body"),
        (
            Call::Mood {
                mood: "reading".into(),
                lasting_us: 1_000_000,
                toward: None,
            },
            "body",
        ),
        (
            Call::Light {
                colour: "warm".into(),
                level: None,
                lasting: None,
                pattern: None,
                keep: false,
            },
            "light",
        ),
        (
            Call::Dim {
                steps: 1,
                until_ends: false,
            },
            "light",
        ),
        (Call::Say("Hello.".into()), "speak"),
        (Call::SayFrom(lamp_skills::Source::Tasks, Some(3)), "speak"),
        (
            Call::Sound {
                name: "ding".into(),
                path: "/x/ding.wav".into(),
            },
            "speak",
        ),
        (Call::Look, "look"),
    ];
    for (call, key) in &calls {
        let why = none.refuses(call).unwrap();
        assert!(why.contains(&format!("needs {key} = true")), "{why}");
        let given = Permissions {
            speak: *key == "speak",
            light: *key == "light",
            body: *key == "body",
            look: *key == "look",
            ..Permissions::default()
        };
        assert_eq!(given.refuses(call), None, "{call:?}");
        // Only that one: every other key's calls are still refused.
        for (other, other_key) in &calls {
            if other_key != key {
                assert!(given.refuses(other).is_some(), "{key} gave {other:?}");
            }
        }
    }
    assert_eq!(none.refuses(&Call::AfterSay), None);
    assert_eq!(none.refuses(&Call::Ends), None);
    assert!(none.refuses_reading("voice/state").is_some());
    let lit = Permissions {
        light: true,
        topics: vec!["voice/state".into()],
        ..Permissions::default()
    };
    assert_eq!(lit.refuses_reading("voice/state"), None);
    assert!(lit.refuses_reading("voice/heard").is_some());
    for name in lamp_skills::LIGHT_ACTIONS {
        assert!(none.refuses_action(name).unwrap().contains("light = true"));
        assert_eq!(lit.refuses_action(name), None);
    }
    assert!(
        lit.refuses_action("volume_up")
            .unwrap()
            .contains("not in [permissions] actions")
    );
    let named = Permissions {
        actions: vec!["volume_up".into()],
        ..Permissions::default()
    };
    assert_eq!(named.refuses_action("volume_up"), None);
    assert!(named.refuses_action("light_colour").is_some());
    assert_eq!(none.said(), "nothing");
    assert_eq!(lit.said(), "reads voice/state; light");
}

/// A WAV of `frames` frames of a steady `level`, as `spec` says.
fn wav(path: &Path, spec: hound::WavSpec, frames: usize, level: i16) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..frames * usize::from(spec.channels) {
        writer.write_sample(level).unwrap();
    }
    writer.finalize().unwrap();
}

/// What the voice plays: 16-bit mono at 24 kHz.
const MONO: hound::WavSpec = hound::WavSpec {
    channels: 1,
    sample_rate: 24_000,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
};

/// A short chime, under the voice.
fn chime(path: &Path) {
    wav(path, MONO, 4_800, 1_000);
}

/// A skill that chimes when someone appears.
const CHIMES: &str = r#"
[trigger]
topic = "humans/faces/tracked"
when = "appears"
[ask]
policy = "hello"
[[do]]
light = { colour = "warm" }
sound = "ding"
"#;

#[test]
fn a_short_sound_loads_and_a_long_loud_or_other_one_is_refused_with_why() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "chime", CHIMES);
    let ding = skill.join("sounds/ding.wav");
    assert_eq!(
        load(&skill).unwrap_err(),
        "sounds/ding.wav: not found",
        "no file yet"
    );
    chime(&ding);
    load(&skill).unwrap();
    for (made, why) in [
        (
            (MONO, 24_000 * 3 + 1, 1_000),
            "not 1 to 72000 frames (3 s at most)",
        ),
        (
            (
                hound::WavSpec {
                    channels: 2,
                    ..MONO
                },
                4_800,
                1_000,
            ),
            "not 16-bit mono PCM at 24000 Hz",
        ),
        (
            (
                hound::WavSpec {
                    sample_rate: 48_000,
                    ..MONO
                },
                4_800,
                1_000,
            ),
            "not 16-bit mono PCM at 24000 Hz",
        ),
        ((MONO, 4_800, 1_100), "louder than -30 dBFS RMS over 10 ms"),
        ((MONO, 4_800, 9_000), "a peak past -12 dBFS"),
    ] {
        let ((spec, frames, level), path) = (made, &ding);
        fs::remove_file(path).unwrap();
        wav(path, spec, frames, level);
        assert_eq!(load(&skill).unwrap_err(), format!("sounds/ding.wav: {why}"));
    }
    fs::remove_file(&ding).unwrap();
    fs::write(&ding, "RIFF, but not").unwrap();
    assert_eq!(load(&skill).unwrap_err(), "sounds/ding.wav: not a WAV file");
    fs::remove_file(&ding).unwrap();
    fs::create_dir(&ding).unwrap();
    assert_eq!(
        load(&skill).unwrap_err(),
        "sounds/ding.wav: not a regular file"
    );
    fs::remove_dir(&ding).unwrap();
    let elsewhere = dir.path().join("elsewhere.wav");
    chime(&elsewhere);
    std::os::unix::fs::symlink(&elsewhere, &ding).unwrap();
    assert_eq!(
        load(&skill).unwrap_err(),
        "sounds/ding.wav: not a regular file"
    );
    fs::remove_file(&ding).unwrap();
    fs::write(&ding, vec![0_u8; 44 + 2 * 72_000 + 4_096 + 1]).unwrap();
    assert!(
        load(&skill)
            .unwrap_err()
            .starts_with("sounds/ding.wav: more than")
    );
    // A name that is no file name, a say beside it, and a tool.
    fs::remove_file(&ding).unwrap();
    chime(&ding);
    for (from, to, why) in [
        (
            r#"sound = "ding""#,
            r#"sound = "../ding""#,
            "not a file name in sounds/",
        ),
        (
            r#"sound = "ding""#,
            "sound = \"ding\"\nsay = \"Hello.\"",
            "one say or sound a skill",
        ),
        (
            "[trigger]",
            "tool = \"chime\"\n[trigger]",
            "sound, in a skill with a tool",
        ),
    ] {
        fs::write(skill.join("skill.toml"), granted(&CHIMES.replace(from, to))).unwrap();
        let error = load(&skill).unwrap_err();
        assert!(error.contains(why), "{to}: {error}");
    }
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// What the program did with one face.
struct Seen {
    events: Vec<SkillEvent>,
    looked: Option<String>,
    /// Each call to the actions part, in order.
    lights: Vec<String>,
    /// When each call to the actions part was answered.
    lit_at: Vec<Instant>,
    /// When the voice wrote each of its answers.
    answered_at: Vec<Instant>,
    /// What the voice was asked to say.
    asked: Option<String>,
    /// What the body's mood socket was told, for a skill that holds a pose.
    moods: Vec<String>,
    /// What was asked on `light/request`, with the ring standing in.
    requests: Vec<LightRequest>,
}

/// How the program is to light (#940).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ring {
    /// Without `LAMP_LIGHT_REQUESTS`: through the actions part.
    Off,
    /// With it, and a ring that takes `light/request`.
    Asked,
    /// With it, and nothing that takes `light/request`.
    Deaf,
}

/// What happens after the faces.
enum After {
    /// Nothing.
    Nothing,
    /// Nobody in view 300 ms after the faces: before it, the light as
    /// someone else set it, if one is given.
    Leaves(Option<Light>),
    /// The skill's folder removed once the first call to the actions part
    /// is answered, and the reload waited for.
    Removed,
}

/// A hello that greets at once, with the light and the say of Welcome back.
const AT_ONCE: &str = r#"
[trigger]
topic = "humans/faces/tracked"
when = "appears"
near = 0.14
[ask]
policy = "hello"
[[do]]
body = { look_at = "face" }
light = { colour = "warm" }
say = "Greet them."
"#;

/// The program on a bus of its own, its skills folder empty until a hello
/// that greets at once is put in it. The light shows blue at 3. One face is
/// published, and the voice's say service answers with `answers`, a line
/// each, 200 ms apart.
fn the_program(skip: bool, answers: &'static [&'static str]) -> Seen {
    the_program_with(AT_ONCE, skip, answers, 3, 1, if skip { 4 } else { 3 })
}

/// The same with this skill, the light blue at `level`, `faces` lists with a
/// face 50 ms apart, until `wanted` events have come. The actions part
/// keeps a light as the real one does, and says it on `light/state`.
fn the_program_with(
    skill: &str,
    skip: bool,
    answers: &'static [&'static str],
    level: u8,
    faces: usize,
    wanted: usize,
) -> Seen {
    the_program_on(skill, skip, answers, level, faces, wanted, Ring::Off)
}

/// The same, lighting as `lighting` says.
fn the_program_on(
    skill: &str,
    skip: bool,
    answers: &'static [&'static str],
    level: u8,
    faces: usize,
    wanted: usize,
    lighting: Ring,
) -> Seen {
    the_program_timed(
        skill,
        skip,
        answers,
        level,
        faces,
        wanted,
        (lighting, After::Nothing),
        false,
    )
}

/// The same, and then, with `then`, nobody in view 300 ms after the faces:
/// before it, the light as someone else set it, if one is given.
fn the_program_then(
    skill: &str,
    skip: bool,
    answers: &'static [&'static str],
    level: u8,
    faces: usize,
    wanted: usize,
    then: Option<Option<Light>>,
) -> Seen {
    the_program_timed(
        skill,
        skip,
        answers,
        level,
        faces,
        wanted,
        (Ring::Off, then.map_or(After::Nothing, After::Leaves)),
        false,
    )
}

/// The program, lighting as `lighting` says and then as `then` says, with
/// `on_the_host` on the lamp's own clock, not a test's.
#[allow(clippy::too_many_arguments)]
fn the_program_timed(
    skill: &str,
    skip: bool,
    answers: &'static [&'static str],
    level: u8,
    faces: usize,
    wanted: usize,
    (lighting, then): (Ring, After),
    on_the_host: bool,
) -> Seen {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("sk")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    fs::create_dir(root.join("skills")).unwrap();
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let gaze = UnixDatagram::bind(root.join("gaze.sock")).unwrap();
    // The body is told to look before the run's done, so by then it is in
    // the socket.
    gaze.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mood = UnixDatagram::bind(root.join("mood.sock")).unwrap();
    mood.set_read_timeout(Some(Duration::from_millis(1_500)))
        .unwrap();
    let clock = if on_the_host {
        Clock::host()
    } else {
        Clock::at(root)
    };
    let actions = UnixListener::bind(root.join("actions.sock")).unwrap();
    let (called, lights) = mpsc::channel();
    let ring = bus.clone();
    std::thread::spawn(move || {
        let mut shown = ring
            .publisher_declared("light/state", &Light::declaration(), "ring")
            .unwrap();
        let mut light = Light::new(true, "blue", level).unwrap();
        for stream in actions.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let call: serde_json::Value = serde_json::from_str(&line).unwrap();
            match call["name"].as_str().unwrap() {
                "light_colour" => {
                    light.on = true;
                    light.colour = call["args"]["colour"].as_str().unwrap().into();
                }
                "light_dimmer" => light.level = light.level.saturating_sub(1).max(1),
                "light_brighter" => light.level = (light.level + 1).min(5),
                "light_off" => light.on = false,
                _ => {}
            }
            shown.publish_json(1, &light).unwrap();
            (&stream)
                .write_all(b"{\"ok\":true,\"result\":{}}\n")
                .unwrap();
            let _ = called.send((line, Instant::now()));
        }
    });
    let voice = UnixListener::bind(root.join("say.sock")).unwrap();
    let (heard, asked) = mpsc::channel();
    let (wrote, written) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in voice.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let _ = heard.send(line);
            for answer in answers {
                std::thread::sleep(Duration::from_millis(200));
                (&stream)
                    .write_all(format!("{answer}\n").as_bytes())
                    .unwrap();
                let _ = wrote.send(Instant::now());
            }
        }
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    command.args([
        "--dir".as_ref(),
        root.join("skills").as_os_str(),
        "--bus-root".as_ref(),
        root.join("bus").as_os_str(),
        "--gaze-socket".as_ref(),
        root.join("gaze.sock").as_os_str(),
        "--mood-socket".as_ref(),
        root.join("mood.sock").as_os_str(),
        "--actions-socket".as_ref(),
        root.join("actions.sock").as_os_str(),
        "--say-socket".as_ref(),
        root.join("say.sock").as_os_str(),
    ]);
    command.args(clock.args());
    command.env_remove("LAMP_SKILLS_ASK");
    if skip {
        command.env("LAMP_SKILLS_ASK", "skip");
    }
    command.env_remove("LAMP_LIGHT_REQUESTS");
    if lighting != Ring::Off {
        command.env("LAMP_LIGHT_REQUESTS", "on");
        command.args(["--boot-id", "boot-test"]);
    }
    let mut requests =
        (lighting == Ring::Asked).then(|| bus.subscribe(LightRequest::TOPIC, "ring").unwrap());
    // Its sound first, then the skill. On a clock the test moves, put in
    // after the start and found by the reload. On the host's clock, there
    // before the start and loaded at once: the reload would cost `RELOAD`,
    // two real seconds (#1128).
    let put_in = || {
        if skill.contains("sound = \"ding\"") {
            chime(&root.join("skills/hello/sounds/ding.wav"));
        }
        skill_with(&root.join("skills"), "hello", skill);
    };
    if on_the_host {
        put_in();
    }
    let _running = Running(command.spawn().unwrap());
    clock.started();
    if !on_the_host {
        put_in();
        clock.skip(RELOAD);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while bus.subscribers(Faces::TOPIC).unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the skill was never loaded");
        std::thread::sleep(Duration::from_millis(50));
    }
    // The light as shown just before the face, fresh.
    bus.publisher_declared("light/state", &Light::declaration(), "ring")
        .unwrap()
        .publish_json(1, &Light::new(true, "blue", level).unwrap())
        .unwrap();
    let mut eyes = bus
        .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
        .unwrap();
    for _ in 0..faces {
        eyes.publish_json(
            1,
            &serde_json::json!({"faces": [{"id": "a1", "xmin": 0.25, "ymin": 0.25,
                "xmax": 0.75, "ymax": 0.5, "confidence": 0.9}]}),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut first = None;
    if let After::Removed = then {
        first = Some(
            lights
                .recv_timeout(Duration::from_secs(5))
                .expect("no call to the actions part before the removal"),
        );
        fs::remove_dir_all(root.join("skills/hello")).unwrap();
        clock.skip(RELOAD);
    }
    if let After::Leaves(changed) = then {
        std::thread::sleep(Duration::from_millis(300));
        if let Some(light) = changed {
            bus.publisher_declared("light/state", &Light::declaration(), "ring")
                .unwrap()
                .publish_json(1, &light)
                .unwrap();
        }
        eyes.publish_json(1, &serde_json::json!({ "faces": [] }))
            .unwrap();
    }
    // A held pose is said when the say begins and then every second of its
    // `for`: a second on at a time, each pose waited for, until a second
    // brings none.
    let mut buffer = [0u8; 512];
    let mut moods = Vec::new();
    while skill.contains("mood =")
        && let Ok(length) = mood.recv(&mut buffer)
    {
        moods.push(String::from_utf8_lossy(&buffer[..length]).into_owned());
        mood.set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        clock.skip(Duration::from_secs(1));
    }
    let mut seen = Vec::new();
    while seen.len() < wanted && Instant::now() < deadline {
        if let Some(message) = events.recv(Duration::from_millis(100)).unwrap() {
            seen.push(message.parse::<SkillEvent>().unwrap());
        }
    }
    let looked = gaze
        .recv(&mut buffer)
        .ok()
        .map(|length| String::from_utf8_lossy(&buffer[..length]).into_owned());
    // And then nothing more.
    while skill.contains("mood =")
        && let Ok(length) = mood.recv(&mut buffer)
    {
        moods.push(String::from_utf8_lossy(&buffer[..length]).into_owned());
    }
    let (lights, lit_at) = first.into_iter().chain(lights.try_iter()).unzip();
    let mut asked_of_the_ring = Vec::new();
    while let Some(Ok(Some(message))) = requests.as_mut().map(|ring| ring.recv(Duration::ZERO)) {
        asked_of_the_ring.push(message.parse().unwrap());
    }
    Seen {
        events: seen,
        looked,
        lights,
        lit_at,
        answered_at: written.try_iter().collect(),
        asked: asked.try_recv().ok(),
        moods,
        requests: asked_of_the_ring,
    }
}

/// A light that dims a step while it greets.
const DIMS: &str = r#"
[trigger]
topic = "humans/faces/tracked"
when = "appears"
[ask]
policy = "hello"
[[do]]
light = { dim = 1 }
say = "Greet them."
"#;

#[test]
fn a_dim_at_the_lowest_level_changes_nothing_and_is_given_back_as_nothing() {
    let seen = the_program_with(
        DIMS,
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
        1,
        1,
        4,
    );
    let done = &seen.events[3].why;
    assert!(
        done.contains("light given back to on blue at 1: it already was"),
        "{done}"
    );
    assert_eq!(seen.lights.len(), 1, "{:?}", seen.lights);
    // At 3 the dim is given back with one step brighter.
    let seen = the_program_with(
        DIMS,
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
        3,
        1,
        4,
    );
    assert!(
        seen.lights[1].contains("light_brighter"),
        "{:?}",
        seen.lights
    );
    assert_eq!(seen.lights.len(), 2, "{:?}", seen.lights);
}

#[test]
fn a_level_is_stepped_to_from_the_level_shown_and_given_back() {
    let to_five = AT_ONCE.replace(
        "light = { colour = \"warm\" }",
        "light = { colour = \"warm\", level = 5 }",
    );
    let seen = the_program_with(
        &to_five,
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
        3,
        1,
        4,
    );
    let names: Vec<String> = seen
        .lights
        .iter()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["name"].to_string())
        .collect();
    // Blue at 3: warm, two steps up to 5, then blue and two steps down to 3.
    assert_eq!(
        names,
        [
            "\"light_colour\"",
            "\"light_brighter\"",
            "\"light_brighter\"",
            "\"light_colour\"",
            "\"light_dimmer\"",
            "\"light_dimmer\""
        ],
        "{:?}",
        seen.lights
    );
    let done = &seen.events[3].why;
    assert!(
        done.contains("light warm: done, to level 5: done, done"),
        "{done}"
    );
    assert!(
        done.contains("light given back to on blue at 3: done, done, done"),
        "{done}"
    );
}

/// #940: with the switch, the light is asked of the ring from the skill
/// and given back by asking with a time already passed; the actions part,
/// which keeps the person's light, is never called.
#[test]
fn with_the_switch_a_skills_light_is_asked_of_the_ring_and_the_persons_never_touched() {
    let to_five = AT_ONCE.replace(
        "light = { colour = \"warm\" }",
        "light = { colour = \"warm\", level = 5 }",
    );
    let words = &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#];
    let seen = the_program_on(&to_five, true, words, 3, 1, 4, Ring::Asked);
    assert!(seen.lights.is_empty(), "{:?}", seen.lights);
    let done = &seen.events[3].why;
    assert!(
        done.contains("light warm: asked of the ring at level 5"),
        "{done}"
    );
    assert!(
        done.contains("say: spoken; light given back: asked again with a time already passed"),
        "{done}"
    );
    assert_eq!(seen.requests.len(), 2, "{:?}", seen.requests);
    let (asked, back) = (&seen.requests[0], &seen.requests[1]);
    assert_eq!(
        (
            asked.claim.source.as_str(),
            asked.claim.priority,
            asked.claim.clock_id.as_str()
        ),
        ("skill:hello", Priority::Skill, "boot-test")
    );
    assert_eq!(asked.light, Light::new(true, "warm", 5).unwrap());
    assert_eq!(back.light, asked.light);
    let now = lamp_ipc::monotonic_us();
    assert!(!back.claim.holds_at("boot-test", now), "{back:?}");
    assert!(asked.claim.until_us > back.claim.until_us);

    // Without a level, the level the light has; a dim steps down from it.
    let seen = the_program_on(AT_ONCE, true, words, 3, 1, 4, Ring::Asked);
    assert_eq!(seen.requests[0].light, Light::new(true, "warm", 3).unwrap());
    let seen = the_program_on(DIMS, true, words, 3, 1, 4, Ring::Asked);
    assert!(seen.lights.is_empty(), "{:?}", seen.lights);
    assert_eq!(seen.requests[0].light, Light::new(true, "blue", 2).unwrap());
}

/// With the switch but no ring that takes requests, the light is left as it
/// is, and the run says why.
#[test]
fn with_the_switch_and_nothing_to_take_it_the_light_is_left_and_the_run_says_why() {
    let words = &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#];
    let seen = the_program_on(AT_ONCE, true, words, 3, 1, 4, Ring::Deaf);
    assert!(seen.lights.is_empty(), "{:?}", seen.lights);
    let done = &seen.events[3].why;
    assert!(
        done.contains("light warm: left alone, since nothing takes light/request (the ring needs LAMP_LIGHT_REQUESTS=on)"),
        "{done}"
    );
    assert!(!done.contains("given back"), "{done}");
    assert!(done.contains("say: spoken"), "{done}");
}

#[test]
fn a_dim_until_it_ends_stays_past_the_says_outcome_until_the_trigger_ends() {
    let until = DIMS.replace("{ dim = 1 }", "{ dim = 1, until = \"ends\" }");
    // The words take a while: two lines between the beginning and the end.
    // Nobody is in view before they end: the dim is given back after both.
    let seen = the_program_then(
        &until,
        true,
        &[
            r#"{"event":"begun","at_us":5}"#,
            r#"{"event":"speaking"}"#,
            r#"{"event":"speaking"}"#,
            r#"{"outcome":"spoken"}"#,
        ],
        3,
        1,
        4,
        Some(None),
    );
    assert_eq!(seen.lights.len(), 2, "{:?}", seen.lights);
    assert!(seen.lights[0].contains("light_dimmer"), "{:?}", seen.lights);
    assert!(
        seen.lights[1].contains("light_brighter"),
        "{:?}",
        seen.lights
    );
    let outcome = *seen.answered_at.last().unwrap();
    assert!(seen.lit_at[0] < outcome && outcome < seen.lit_at[1]);
    let done = &seen.events[3].why;
    assert!(
        done.contains(
            "say: spoken; held until its trigger ended; light given back to on blue at 3: done"
        ),
        "{done}"
    );
}

#[test]
fn a_skill_runs_once_at_a_time_and_says_so_when_it_cannot() {
    // Present and no cooldown: every list with a face triggers it, while the
    // first say still waits on the voice.
    let present = AT_ONCE.replace("when = \"appears\"", "when = \"present\"");
    let seen = the_program_with(
        &present,
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
        3,
        3,
        12,
    );
    let refused = seen
        .events
        .iter()
        .filter(|event| event.why == "not made: its last run is still under way")
        .count();
    assert_eq!(refused, 2, "{:?}", seen.events);
}

fn stages(events: &[SkillEvent]) -> Vec<SkillStage> {
    events.iter().map(|event| event.stage).collect()
}

#[test]
fn the_program_looks_and_lights_with_the_words_and_gives_the_light_back() {
    let seen = the_program(
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
    );
    assert_eq!(
        stages(&seen.events),
        [
            SkillStage::Triggered,
            SkillStage::Asked,
            SkillStage::Answered,
            SkillStage::Done
        ]
    );
    assert_eq!(seen.events[2].allowed, Some(true));
    let done = &seen.events[3].why;
    assert!(done.contains("look at the face: told the body"), "{done}");
    assert!(done.contains("light warm: done"), "{done}");
    assert!(done.contains("say: spoken"), "{done}");
    assert!(done.contains("light given back to on blue at 3"), "{done}");
    assert_eq!(
        seen.asked.as_deref(),
        Some(
            "{\"expires_in_ms\":30000,\"note\":\"Greet them.\",\"priority\":5,\"skill\":\"hello\"}\n"
        )
    );
    assert_eq!(
        seen.looked.as_deref(),
        Some(r#"{"face":{"size":0.25,"turned":0.0,"x":0.0,"y":-0.25}}"#)
    );
    assert_eq!(
        seen.lights,
        [
            "{\"args\":{\"colour\":\"warm\"},\"name\":\"light_colour\",\"op\":\"call\"}\n",
            "{\"args\":{\"colour\":\"blue\"},\"name\":\"light_colour\",\"op\":\"call\"}\n"
        ]
    );
}

/// A skill whose words the voice keeps (#939, (c)): the reminder due.
const FROM_SCHEDULE: &str = r#"
[trigger]
topic = "humans/faces/tracked"
when = "appears"
[ask]
policy = "hello"
[[do]]
light = { colour = "warm" }
say = { from = "schedule" }
"#;

/// #939, (c): `say = { from = "schedule" }` asks the voice for what it
/// keeps, at a reminder's priority, with no words of the skill's own; the
/// light comes with its `begun` and is given back, as for a note.
#[test]
fn a_say_from_the_voices_own_asks_for_it_by_source_and_moves_with_it() {
    let seen = the_program_with(
        FROM_SCHEDULE,
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
        3,
        1,
        4,
    );
    assert_eq!(
        seen.asked.as_deref(),
        Some(
            "{\"expires_in_ms\":30000,\"from\":\"schedule\",\"priority\":8,\"skill\":\"hello\"}\n"
        )
    );
    let done = &seen.events[3].why;
    assert!(done.contains("light warm: done"), "{done}");
    assert!(done.contains("say: spoken"), "{done}");
    assert!(done.contains("light given back to on blue at 3"), "{done}");
}

/// #938: the sound is asked of the voice's say service by its absolute
/// path, the light comes with its `begun`, and what the voice says of it is
/// in the run's `done`.
#[test]
fn a_sound_is_asked_of_the_voice_and_its_refusal_is_said_in_the_event() {
    let seen = the_program_with(
        CHIMES,
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
        3,
        1,
        4,
    );
    let asked: serde_json::Value = serde_json::from_str(seen.asked.as_deref().unwrap()).unwrap();
    assert_eq!(asked["skill"], "hello");
    assert_eq!(asked["priority"], 5);
    let path = asked["sound"].as_str().unwrap();
    assert!(
        path.starts_with('/') && path.ends_with("/skills/hello/sounds/ding.wav"),
        "{path}"
    );
    assert!(asked.get("note").is_none());
    let done = &seen.events[3].why;
    assert!(done.contains("light warm: done"), "{done}");
    assert!(done.contains("sound: spoken"), "{done}");
    assert!(done.contains("light given back to on blue at 3"), "{done}");
    let refused = the_program_with(
        CHIMES,
        true,
        &[r#"{"outcome":"refused","why":"the lamp is speaking"}"#],
        3,
        1,
        4,
    );
    assert_eq!(
        refused.events[3].why,
        "sound: refused (the lamp is speaking)"
    );
    assert_eq!(refused.lights.len(), 0, "nothing made with a sound refused");
}

#[test]
fn a_say_never_begun_moves_nothing_and_changes_no_light() {
    let seen = the_program(true, &[r#"{"outcome":"expired"}"#]);
    assert_eq!(stages(&seen.events).last(), Some(&SkillStage::Done));
    let done = &seen.events[3].why;
    assert_eq!(done, "say: expired");
    assert!(seen.asked.is_some());
    assert_eq!((seen.looked, seen.lights.len()), (None, 0));
}

#[test]
fn the_program_does_nothing_without_the_skip() {
    let seen = the_program(false, &[]);
    assert_eq!(
        stages(&seen.events),
        [
            SkillStage::Triggered,
            SkillStage::Asked,
            SkillStage::Answered
        ]
    );
    assert_eq!(seen.events[2].allowed, Some(false));
    assert_eq!(
        (seen.looked, seen.lights.len(), seen.asked),
        (None, 0, None)
    );
}

/// Everything the tests' own skills read and use, for the tests of other
/// things than permissions: a skill without `[permissions]` may do nothing.
const GRANTED: &str = "\n[permissions]\n\
                       topics = [\"air/indoor\", \"memory/rules\", \"air/state\", \"humans/faces/tracked\", \"schedule/due\", \
                       \"tasks/status\", \"voice/heard\", \"voice/state\"]\n\
                       speak = true\nlight = true\nbody = true\nlook = true\n";

/// `toml` with [`GRANTED`], unless it says its own `[permissions]`.
fn granted(toml: &str) -> String {
    if toml.contains("[permissions]") {
        toml.to_owned()
    } else {
        format!("{toml}{GRANTED}")
    }
}

/// A skill folder named `name` in `dir` with this `skill.toml`, given
/// everything it uses unless it says its own `[permissions]`.
fn skill_with(dir: &Path, name: &str, toml: &str) -> PathBuf {
    let skill = dir.join(name);
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A test.\n---\n"),
    )
    .unwrap();
    fs::write(skill.join("skill.toml"), granted(toml)).unwrap();
    skill
}

const FACE: &str =
    r#"{"faces":[{"id":"a","xmin":0.25,"ymin":0.25,"xmax":0.75,"ymax":0.5,"confidence":0.9}]}"#;
const NOBODY: &str = r#"{"faces":[]}"#;

/// Stretch break as the skill creator wrote it (#947), its condition with
/// the field named.
const STRETCH: &str = r#"
needs = ["eyes", "body", "light"]
[trigger]
topic = "humans/faces/tracked"
when = "present"
for = "50m"
gap = "2m"
rearm = "leaves"
cooldown = "90m"
[[while]]
topic = "voice/state"
field = "state"
equals = "resting"
[ask]
policy = "nudge"
[[do]]
body = { look_at = "face" }
[[do]]
body = { gesture = "stretching" }
[[do]]
light = { colour = "warm", level = 2, for = "4s" }
"#;

/// Messages as (seconds, topic, payload); the seconds of each run and its calls.
fn runs_at(skill: &Path, messages: &[(u64, &str, &str)]) -> Vec<(u64, Vec<Call>)> {
    let mut engine = Engine::new(vec![load(skill).unwrap()], true);
    let mut runs = Vec::new();
    for (at, topic, payload) in messages {
        for run in engine.observe(topic, at * 1_000_000, payload.as_bytes(), None) {
            runs.push((*at, run.calls));
        }
    }
    runs
}

/// Someone at the desk each minute from `from` to `to` seconds, gone for
/// 30 s after each of `away`.
fn sitting(from: u64, to: u64, away: &[u64]) -> Vec<(u64, &'static str, &'static str)> {
    let mut messages = Vec::new();
    for at in (from..=to).step_by(60) {
        messages.push((at, Faces::TOPIC, FACE));
        if away.contains(&at) {
            messages.push((at + 30, Faces::TOPIC, NOBODY));
        }
    }
    messages
}

/// #940 item 3: a light file's pattern is loaded into its call, its kind's
/// period when it names none and a low of 1; a kind the ring does not play,
/// a pattern without the light's level, a low at the level or a period out
/// of bounds are refused when the skill loads.
#[test]
fn a_light_files_pattern_is_loaded_and_held_to_its_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let with_light = |name: &str, light: &str| {
        let skill = skill_with(
            dir.path(),
            name,
            &STRETCH.replace(
                r#"light = { colour = "warm", level = 2, for = "4s" }"#,
                r#"light = "glow""#,
            ),
        );
        fs::create_dir_all(skill.join("lights")).unwrap();
        fs::write(skill.join("lights/glow.toml"), light).unwrap();
        skill
    };
    let skill = with_light(
        "calm",
        "colour = \"warm\"\nlevel = 3\nfor = \"4s\"\npattern = { kind = \"breathe\" }\n",
    );
    let mut messages = vec![(0, "voice/state", r#"{"state":"resting"}"#)];
    messages.extend(sitting(0, 3300, &[600, 1800]));
    messages.sort_by_key(|message| message.0);
    let runs = runs_at(&skill, &messages);
    assert_eq!(
        runs[0].1[2],
        Call::Light {
            colour: "warm".into(),
            level: Some(3),
            lasting: Some("4s".into()),
            pattern: Some(LightPattern {
                kind: PatternKind::Breathe,
                period_ms: LightPattern::BREATHE_DEFAULT_MS,
                low: 1,
            }),
            keep: false,
        }
    );
    let candle = with_light(
        "fire",
        "colour = \"orange\"\nlevel = 4\npattern = { kind = \"candle\", period = \"2s\" }\n",
    );
    load(&candle).unwrap();
    for (name, light, why) in [
        (
            "blinks",
            "colour = \"warm\"\nlevel = 3\npattern = { kind = \"blink\" }\n",
            "not breathe, candle or pulse",
        ),
        (
            "levelless",
            "colour = \"warm\"\npattern = { kind = \"breathe\" }\n",
            "needs the light's level",
        ),
        (
            "flat",
            "colour = \"warm\"\nlevel = 3\npattern = { kind = \"breathe\", low = 3 }\n",
            "under the light's level",
        ),
        (
            "low-candle",
            "colour = \"warm\"\nlevel = 4\npattern = { kind = \"candle\", low = 2 }\n",
            "only a breath takes low",
        ),
        (
            "dim-candle",
            "colour = \"warm\"\nlevel = 2\npattern = { kind = \"candle\" }\n",
            "a candle flickers at level 3 and up",
        ),
        (
            "anxious",
            "colour = \"warm\"\nlevel = 3\npattern = { kind = \"breathe\", period = \"3s\" }\n",
            "4000 to 12000 ms",
        ),
    ] {
        let error = load(&with_light(name, light)).unwrap_err();
        assert!(error.contains(why), "{name}: {error}");
    }
}

#[test]
fn present_for_a_while_through_short_dropouts_once_a_sitting_and_only_while_resting() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "stretch-break", STRETCH);
    let resting = (0, "voice/state", r#"{"state":"resting"}"#);
    let mut messages = vec![resting];
    messages.extend(sitting(0, 3300, &[600, 1800]));
    // They leave for five minutes and sit 55 minutes more: within the
    // cooldown, nothing.
    messages.push((3400, Faces::TOPIC, NOBODY));
    messages.extend(sitting(3700, 7000, &[]));
    messages.sort_by_key(|message| message.0);
    let runs = runs_at(&skill, &messages);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].0, 3000);
    assert_eq!(
        runs[0].1,
        [
            Call::Gaze {
                face: Some((0.0, -0.25, 0.25))
            },
            Call::Gesture("stretching".into()),
            Call::Light {
                colour: "warm".into(),
                level: Some(2),
                lasting: Some("4s".into()),
                pattern: None,
                keep: false,
            }
        ]
    );
    // Talking at the fiftieth minute: its chance this sitting is gone, and
    // the next sitting, with no cooldown begun, has its own.
    let mut talking = messages.clone();
    talking.push((2950, "voice/state", r#"{"state":"speaking"}"#));
    talking.push((3100, "voice/state", r#"{"state":"resting"}"#));
    talking.sort_by_key(|message| message.0);
    let runs = runs_at(&skill, &talking);
    assert_eq!(runs.iter().map(|run| run.0).collect::<Vec<_>>(), [6700]);
    // A dropout longer than the gap begins the sitting again: 50 minutes
    // from 1,320 s is past the end.
    let mut broken: Vec<_> = sitting(0, 3300, &[])
        .into_iter()
        .filter(|message| !(1000..1300).contains(&message.0))
        .collect();
    broken.push(resting);
    broken.push((1001, Faces::TOPIC, NOBODY));
    broken.sort_by_key(|message| message.0);
    assert!(runs_at(&skill, &broken).is_empty());
}

/// #1021's rule for every skill, Stretch break's case: someone talking
/// when the moment comes, and the lamp acts once the talk ends, within the
/// same sitting and its `wait`, instead of spending its chance.
#[test]
fn a_chance_held_back_by_a_talk_waits_for_its_end_within_the_wait() {
    let dir = tempfile::tempdir().unwrap();
    let waiting = STRETCH.replace("cooldown = \"90m\"", "cooldown = \"90m\"\nwait = \"10m\"");
    let skill = skill_with(dir.path(), "stretch-break", &waiting);
    let mut messages = vec![(0, "voice/state", r#"{"state":"resting"}"#)];
    messages.extend(sitting(0, 4000, &[]));
    // Talking from before the fiftieth minute to 3,100 s: it stretches at
    // the first face after the talk, once.
    let mut talking = messages.clone();
    talking.push((2950, "voice/state", r#"{"state":"speaking"}"#));
    talking.push((3100, "voice/state", r#"{"state":"resting"}"#));
    talking.sort_by_key(|message| message.0);
    let runs = runs_at(&skill, &talking);
    assert_eq!(runs.iter().map(|run| run.0).collect::<Vec<_>>(), [3120]);
    // A talk that outlasts the wait (from 3,000 s, ten minutes, to 3,600 s):
    // the chance is spent, as without one.
    let mut long = messages.clone();
    long.push((2950, "voice/state", r#"{"state":"speaking"}"#));
    long.push((3700, "voice/state", r#"{"state":"resting"}"#));
    long.sort_by_key(|message| message.0);
    assert!(
        runs_at(&skill, &long).is_empty(),
        "{:?}",
        runs_at(&skill, &long)
    );
    // They leave during the talk, longer than the gap: the sitting and its
    // chance are over, and the next sitting has 50 minutes of its own.
    let mut gone: Vec<_> = talking
        .iter()
        .copied()
        .filter(|message| !(message.1 == Faces::TOPIC && (3000..3300).contains(&message.0)))
        .collect();
    gone.push((3001, Faces::TOPIC, NOBODY));
    gone.sort_by_key(|message| message.0);
    assert!(
        runs_at(&skill, &gone).is_empty(),
        "{:?}",
        runs_at(&skill, &gone)
    );
    // Without `wait`, as before: the chance is spent.
    let plain = skill_with(&dir.path().join("plain"), "stretch-break", STRETCH);
    assert!(runs_at(&plain, &talking).is_empty());
    // `wait` needs a chance to keep, and is bounded.
    for (toml, why) in [
        (
            waiting.replace("rearm = \"leaves\"\n", ""),
            "wait goes with rearm",
        ),
        (
            waiting.replace("wait = \"10m\"", "wait = \"2h\""),
            "at most 1h",
        ),
    ] {
        let error = load(&skill_with(&dir.path().join("bad"), "stretch-break", &toml)).unwrap_err();
        assert!(error.contains(why), "{error}");
    }
}

#[test]
fn a_field_above_a_line_for_a_while_once_a_spell_and_only_with_someone_there() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(
        dir.path(),
        "stuffy-air",
        r#"
needs = ["air", "eyes", "light"]
[trigger]
topic = "air/state"
field = "co2_ppm"
above = 1200
for = "10m"
rearm = "ends"
cooldown = "1h"
[[while]]
topic = "humans/faces/tracked"
present = true
[ask]
policy = "nudge"
[[do]]
light = { dim = 1, until = "ends" }
"#,
    );
    let high = r#"{"co2_ppm":1300,"feel":"stuffy"}"#;
    let low = r#"{"co2_ppm":800,"feel":"fresh"}"#;
    let mut messages = Vec::new();
    for at in (0..=1500).step_by(60) {
        messages.push((at, Faces::TOPIC, FACE));
        messages.push((at, "air/state", high));
    }
    let runs = runs_at(&skill, &messages);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0].0, 600);
    assert_eq!(
        runs[0].1,
        [Call::Dim {
            steps: 1,
            until_ends: true
        }]
    );
    // Nobody at the desk: nothing.
    let alone: Vec<_> = messages
        .iter()
        .filter(|message| message.1 != Faces::TOPIC)
        .copied()
        .collect();
    assert!(runs_at(&skill, &alone).is_empty());
    // A rise of five minutes: nothing.
    let mut short = Vec::new();
    for at in (0..=1500).step_by(60) {
        short.push((at, Faces::TOPIC, FACE));
        short.push((at, "air/state", if at < 300 { high } else { low }));
    }
    assert!(runs_at(&skill, &short).is_empty());
}

#[test]
fn an_expired_face_is_not_looked_at_and_the_other_steps_still_run() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(
        dir.path(),
        "stuffy-look",
        r#"
[trigger]
topic = "air/state"
field = "co2_ppm"
above = 1200
for = "10m"
rearm = "ends"
[ask]
policy = "nudge"
[[do]]
body = { look_at = "face" }
light = { dim = 1 }
"#,
    );
    let high = r#"{"co2_ppm":1300,"feel":"stuffy"}"#;
    let mut messages: Vec<_> = (0..=600)
        .step_by(60)
        .map(|at| (at, "air/state", high))
        .collect();
    // A face seen once, at the start: its two seconds are long over at 600 s.
    messages.insert(0, (0, Faces::TOPIC, FACE));
    let dim = Call::Dim {
        steps: 1,
        until_ends: false,
    };
    assert_eq!(runs_at(&skill, &messages), [(600, vec![dim.clone()])]);
    // The same face seen at 600 s, just before the air: looked at.
    messages.insert(messages.len() - 1, (600, Faces::TOPIC, FACE));
    let gaze = Call::Gaze {
        face: Some((0.0, -0.25, 0.25)),
    };
    assert_eq!(runs_at(&skill, &messages), [(600, vec![gaze, dim])]);
}

#[test]
fn a_look_at_a_face_subscribes_to_the_faces_whatever_the_trigger() {
    let dir = tempfile::tempdir().unwrap();
    let skill = |name: &str, step: &str| {
        let toml = format!(
            "[trigger]\ntopic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\n\
             [ask]\npolicy = \"nudge\"\n[[do]]\n{step}\n"
        );
        load(&skill_with(dir.path(), name, &toml)).unwrap()
    };
    let looks = Engine::new(vec![skill("looks", "body = { look_at = \"face\" }")], true);
    assert_eq!(looks.topics(), ["air/state", Faces::TOPIC]);
    let dims = Engine::new(vec![skill("dims", "light = { dim = 1 }")], true);
    assert_eq!(dims.topics(), ["air/state"]);
}

/// Air above the line, with a condition that someone has been at the desk
/// for 30 s.
fn sat_for_thirty(dir: &Path, name: &str, condition: &str) -> PathBuf {
    skill_with(
        dir,
        name,
        &format!(
            "[trigger]\ntopic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\n\
             [[while]]\n{condition}\nfor = \"30s\"\n\
             [ask]\npolicy = \"nudge\"\n[[do]]\nlight = {{ dim = 1 }}\n"
        ),
    )
}

#[test]
fn a_timed_condition_begins_again_after_its_value_expired() {
    let dir = tempfile::tempdir().unwrap();
    let skill = sat_for_thirty(
        dir.path(),
        "sat",
        "topic = \"humans/faces/tracked\"\npresent = true",
    );
    let high = r#"{"co2_ppm":1300,"feel":"stuffy"}"#;
    // A face at 0 s, its list lasting 2 s, then nothing until 31 s: the 29 s
    // between are not 30 s of someone there.
    let gap = [
        (0, Faces::TOPIC, FACE),
        (31, Faces::TOPIC, FACE),
        (31, "air/state", high),
    ];
    assert!(runs_at(&skill, &gap).is_empty());
    // A face every second for 31 s is.
    let mut held: Vec<_> = (0..=31).map(|at| (at, Faces::TOPIC, FACE)).collect();
    held.push((31, "air/state", high));
    assert_eq!(runs_at(&skill, &held).len(), 1);
}

#[test]
fn a_not_equals_condition_holds_while_the_field_is_anything_else_for_its_time() {
    let dir = tempfile::tempdir().unwrap();
    let skill = sat_for_thirty(
        dir.path(),
        "unspoken",
        "topic = \"voice/state\"\nfield = \"state\"\nnot_equals = \"speaking\"",
    );
    let high = r#"{"co2_ppm":1300,"feel":"stuffy"}"#;
    let state = |text: &str| format!("{{\"state\":\"{text}\"}}");
    let (speaking, listening, resting) = (state("speaking"), state("listening"), state("resting"));
    // The lamp spoke at 0 s; then the room's talk and rest, never the lamp,
    // for 31 s: 30 s without speaking.
    let mut quiet: Vec<(u64, &str, &str)> = vec![(0, "voice/state", speaking.as_str())];
    quiet.extend((1..=31).map(|at| {
        let value = if at % 3 == 0 {
            listening.as_str()
        } else {
            resting.as_str()
        };
        (at, "voice/state", value)
    }));
    quiet.push((31, "air/state", high));
    assert_eq!(runs_at(&skill, &quiet).len(), 1);
    // The same, but the lamp speaking again at 10 s: 21 s is not 30 s.
    let mut spoke: Vec<(u64, &str, &str)> = quiet.clone();
    spoke[10] = (10, "voice/state", speaking.as_str());
    assert!(runs_at(&skill, &spoke).is_empty());
    // not_equals stands alone: with equals it is refused.
    let both = skill_with(
        dir.path(),
        "both",
        "[trigger]\ntopic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\n\
         [[while]]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\nnot_equals = \"speaking\"\n\
         [ask]\npolicy = \"nudge\"\n[[do]]\nlight = { dim = 1 }\n",
    );
    assert!(load(&both).unwrap_err().contains("not_equals"));
}

/// #1677: `unknown = "holds"` holds while its topic has said nothing yet
/// (a lamp without the rules), never on a value that says otherwise, and
/// not on one that has run out; left out, a silent topic holds nothing.
#[test]
fn an_unknown_holds_condition_holds_only_while_its_topic_has_said_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let toml = |unknown: &str| {
        format!(
            "[trigger]\ntopic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\n\
             [[while]]\ntopic = \"memory/rules\"\nfield = \"keep_light_on\"\nnot_equals = true\n{unknown}\
             [ask]\npolicy = \"nudge\"\n[[do]]\nlight = {{ dim = 1 }}\n"
        )
    };
    let holds = skill_with(dir.path(), "holds", &toml("unknown = \"holds\"\n"));
    let plain = skill_with(dir.path(), "plain", &toml(""));
    let high = r#"{"co2_ppm":1300,"feel":"stuffy"}"#;
    // Nothing on memory/rules: only the skill asked to hold runs.
    assert_eq!(runs_at(&holds, &[(1, "air/state", high)]).len(), 1);
    assert!(runs_at(&plain, &[(1, "air/state", high)]).is_empty());
    // Asked to keep the light on: neither runs; not asked: both do.
    for (rules, runs) in [
        (r#"{"keep_light_on":true}"#, 0),
        (r#"{"keep_light_on":false}"#, 1),
    ] {
        for skill in [&holds, &plain] {
            let messages = [(0, "memory/rules", rules), (1, "air/state", high)];
            assert_eq!(runs_at(skill, &messages).len(), runs, "{rules}");
        }
    }
    // A value that has run out is not "said nothing": it holds no more.
    let mut engine = Engine::new(vec![load(&holds).unwrap()], true);
    engine.observe(
        "memory/rules",
        0,
        br#"{"keep_light_on":false}"#,
        Some(1_000),
    );
    assert!(
        engine
            .observe("air/state", 5_000_000, high.as_bytes(), None)
            .is_empty()
    );
    // With for or present it is refused, and anything but "holds" is.
    for (extra, why) in [
        (
            "unknown = \"holds\"\nfor = \"10s\"\n",
            "without for or present",
        ),
        ("unknown = \"always\"\n", "unknown is \"holds\""),
    ] {
        let bad = skill_with(dir.path(), "bad", &toml(extra));
        assert!(load(&bad).unwrap_err().contains(why), "{extra}");
    }
}

#[test]
fn a_condition_changed_by_a_reload_begins_again() {
    let dir = tempfile::tempdir().unwrap();
    let speaking = sat_for_thirty(
        dir.path(),
        "talk",
        "topic = \"voice/state\"\nfield = \"state\"\nequals = \"speaking\"",
    );
    let mut engine = Engine::new(vec![load(&speaking).unwrap()], true);
    let state = |word: &str| format!(r#"{{"state":"{word}"}}"#);
    let high = br#"{"co2_ppm":1300,"feel":"stuffy"}"#;
    // Speaking from 0 s: the same condition, reloaded, keeps its time.
    engine.observe("voice/state", 0, state("speaking").as_bytes(), None);
    engine.replace(vec![load(&speaking).unwrap()]);
    assert_eq!(engine.observe("air/state", 31_000_000, high, None).len(), 1);
    // The same slot edited to resting: its first resting sample at 40 s
    // does not inherit 0 s.
    let mut engine = Engine::new(vec![load(&speaking).unwrap()], true);
    engine.observe("voice/state", 0, state("speaking").as_bytes(), None);
    let resting = sat_for_thirty(
        &dir.path().join("edited"),
        "talk",
        "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"",
    );
    engine.replace(vec![load(&resting).unwrap()]);
    engine.observe("voice/state", 40_000_000, state("resting").as_bytes(), None);
    assert!(
        engine
            .observe("air/state", 41_000_000, high, None)
            .is_empty()
    );
    assert_eq!(engine.observe("air/state", 71_000_000, high, None).len(), 1);
}

#[test]
fn a_folder_beginning_with_a_dot_is_passed_over() {
    let dir = tempfile::tempdir().unwrap();
    skill_with(dir.path(), ".draft", "not toml at all");
    welcome_back_with(dir.path(), "needs", "needs");
    let (skills, errors) = load_all(dir.path());
    assert_eq!(skills.len(), 1);
    assert!(errors.is_empty(), "{errors:?}");
}

/// A skill folder copied from a Mac (`tar` without `COPYFILE_DISABLE=1`)
/// has a binary `._NAME` beside each file, and `lamp-skills test` failed on
/// them on lamp-4ace (2026-10-16): they are passed over, and the skill loads
/// and passes as before.
#[test]
fn a_macs_appledouble_files_are_passed_over() {
    let dir = tempfile::tempdir().unwrap();
    let skill = welcome_back_with(dir.path(), "cooldown = \"20m\"", "cooldown = \"20m\"");
    let clean = run_tests(&skill).unwrap();
    let binary = [0u8, 5, 22, 7, 0, 2, 0, 0, 0xff, 0xfe, 0x80];
    let mut beside = Vec::new();
    let mut folders = vec![skill.clone()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path.clone());
            }
            let name = format!("._{}", entry.file_name().to_string_lossy());
            beside.push(folder.join(name));
        }
    }
    for path in &beside {
        fs::write(path, binary).unwrap();
    }
    assert!(
        beside
            .iter()
            .any(|path| path.to_string_lossy().ends_with(".jsonl"))
    );
    load(&skill).unwrap();
    assert_eq!(run_tests(&skill).unwrap(), clean);
}

/// Desk tutor's shape (#941): started by its tool alone, it lights the
/// page and has the voice take its full look.
const TUTOR: &str = r#"
needs = ["eyes", "voice", "light"]
tool = "check_my_work"
[[do]]
light = { colour = "white", for = "1s" }
look = true
say = "Tutor: a hint or a question, never the answer."
"#;

#[test]
fn a_reading_pose_is_held_on_the_mood_socket_for_its_for_and_then_let_go() {
    let reading = AT_ONCE.replace(
        "body = { look_at = \"face\" }",
        "body = { mood = \"reading\", for = \"1500ms\" }",
    );
    // On the lamp's own clock, as a lamp runs it: the pose said again on
    // the host's seconds. 1.5 s, the shortest `for` that is still said
    // again: one second on, and half a second short of the next.
    let seen = the_program_timed(
        &reading,
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
        3,
        1,
        4,
        (Ring::Off, After::Nothing),
        true,
    );
    // Told at once when the say begins, then every second while it lasts:
    // two in all for 1.5 s, and then nothing, so the body comes back.
    assert_eq!(
        seen.moods,
        vec![r#"{"level":0,"mood":"reading"}"#; 2],
        "{:?}",
        seen.events
    );
    let dir = tempfile::tempdir().unwrap();
    for (step, why) in [
        ("body = { mood = \"dance\", for = \"3s\" }", "not one of"),
        ("body = { mood = \"reading\" }", "goes with its for"),
        ("body = { mood = \"reading\", for = \"3m\" }", "at most 2m"),
    ] {
        let toml = AT_ONCE.replace("body = { look_at = \"face\" }", step);
        let error = load(&skill_with(&dir.path().join("bad"), "hello", &toml)).unwrap_err();
        assert!(error.contains(why), "{step}: {error}");
    }
}

/// A case of a skill a person starts (#941, Desk tutor): `[[call_tool]]`
/// makes the model's call at its time, after the messages recorded before
/// it, its conditions judged on them.
#[test]
fn a_case_calls_the_skills_tool_at_its_time_and_needs_no_recording_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let toml = TUTOR.replace(
        "[[do]]",
        "[[while]]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n[[do]]",
    );
    let skill = skill_with(dir.path(), "tutor", &toml);
    let case = skill.join("tests/asked");
    fs::create_dir_all(&case).unwrap();
    // voice/state: resting at 0, speaking at 10 s.
    let header = |seq: u64, at: u64| {
        format!(
            r#"{{"header":{{"topic":"voice/state","kind":"latest","enc":"json","type":"lamp_msgs/VoiceState","source":"test","boot":"b","seq":{seq},"at_us":{at},"at_unix_ms":{}}},"payload_b64":"{}"}}"#,
            1_792_000_000_000 + at / 1_000,
            if seq == 1 {
                "eyJzdGF0ZSI6InJlc3RpbmcifQ=="
            } else {
                "eyJzdGF0ZSI6InNwZWFraW5nIn0="
            }
        )
    };
    fs::write(
        case.join("voice_state.jsonl"),
        format!(
            "{}\n{}\n{}\n",
            r#"{"topic":"voice/state","declaration":{"type":"lamp_msgs/VoiceState","version":1,"kind":"latest"}}"#,
            header(1, 0),
            header(2, 10_000_000)
        ),
    )
    .unwrap();
    let expect = |text: &str| fs::write(case.join("expect.toml"), text).unwrap();
    // Called at 5 s, while resting: its steps, at 5 s.
    expect(
        r#"
[[call_tool]]
name = "check_my_work"
at = "5s"
[[expect]]
at = "5s"
call = "look"
[[expect]]
at = "5s"
call = "light"
args = { colour = "white" }
[[expect]]
call = "say"
"#,
    );
    assert_eq!(run_tests(&skill).unwrap(), (1, 0));
    // Called at 12 s, after the lamp began speaking at 10 s: refused, nothing.
    expect("none = true\n[[call_tool]]\nname = \"check_my_work\"\nat = \"12s\"\n");
    assert_eq!(run_tests(&skill).unwrap(), (1, 0));
    // The same, expecting its steps, fails: the call came after the message.
    expect("[[call_tool]]\nname = \"check_my_work\"\nat = \"12s\"\n[[expect]]\ncall = \"look\"\n");
    assert!(run_tests(&skill).unwrap_err().contains("made 0 calls"));
    // A case that is only a call needs no recording, when nothing holds it back.
    let plain = skill_with(&dir.path().join("plain"), "tutor", TUTOR);
    let only = plain.join("tests/only");
    fs::create_dir_all(&only).unwrap();
    fs::write(
        only.join("expect.toml"),
        "[[call_tool]]\nname = \"check_my_work\"\nat = \"1s\"\n[[expect]]\ncall = \"look\"\n[[expect]]\ncall = \"light\"\n[[expect]]\ncall = \"say\"\n",
    )
    .unwrap();
    assert_eq!(run_tests(&plain).unwrap(), (1, 0));
    // A name that is not the skill's tool fails the case, not passes it.
    fs::write(
        only.join("expect.toml"),
        "none = true\n[[call_tool]]\nname = \"check_my_wrok\"\nat = \"1s\"\n",
    )
    .unwrap();
    let why = run_tests(&plain).unwrap_err();
    assert!(
        why.contains("this skill's tool is \"check_my_work\""),
        "{why}"
    );
}

#[test]
fn a_tool_needs_no_trigger_and_a_look_needs_a_tool() {
    let dir = tempfile::tempdir().unwrap();
    let skill = load(&skill_with(dir.path(), "tutor", TUTOR)).unwrap();
    assert_eq!(skill.tool.as_deref(), Some("check_my_work"));
    assert!(skill.triggers.is_empty());
    for (toml, why) in [
        (TUTOR.replace("check_my_work", "Check My Work"), "tool \""),
        (TUTOR.replace("tool = \"check_my_work\"", ""), "a trigger or a tool"),
        (
            TUTOR.replace(
                "tool = \"check_my_work\"",
                "tool = \"check_my_work\"\n[trigger]\ntopic = \"humans/faces/tracked\"\n[ask]\npolicy = \"nudge\"",
            ),
            "started by its tool alone",
        ),
        // What the voice keeps is never said from the model's call: a tool's
        // run would drop it without a word (Fourth on #1033).
        (
            TUTOR.replace(
                "say = \"Tutor: a hint or a question, never the answer.\"",
                "say = { from = \"schedule\" }",
            ),
            "say.from, in a skill with a tool",
        ),
    ] {
        let error = load(&skill_with(&dir.path().join("bad"), "tutor", &toml)).unwrap_err();
        assert!(error.contains(why), "{toml}: {error}");
    }
}

#[test]
fn a_tools_call_runs_only_while_its_conditions_hold_on_values_that_last() {
    let dir = tempfile::tempdir().unwrap();
    let toml = TUTOR.replace(
        "[[do]]",
        "[[while]]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\nfor = \"30s\"\n[[do]]",
    );
    let mut engine = Engine::new(
        vec![load(&skill_with(dir.path(), "tutor", &toml)).unwrap()],
        false,
    );
    let s = 1_000_000;
    let resting = br#"{"state":"resting"}"#;
    // Never seen.
    assert!(
        engine
            .tool("check_my_work", &serde_json::Value::Null, 0)
            .is_err()
    );
    engine.observe("voice/state", 0, resting, None);
    // Not yet for its 30 s, then it has.
    assert!(
        engine
            .tool("check_my_work", &serde_json::Value::Null, 10 * s)
            .is_err()
    );
    assert!(
        engine
            .tool("check_my_work", &serde_json::Value::Null, 31 * s)
            .is_ok()
    );
    // False.
    engine.observe("voice/state", 40 * s, br#"{"state":"speaking"}"#, None);
    assert!(
        engine
            .tool("check_my_work", &serde_json::Value::Null, 41 * s)
            .is_err()
    );
    // A value lasting 60 s: held 30 s within it, then expired.
    engine.observe("voice/state", 100 * s, resting, Some(60_000));
    assert!(
        engine
            .tool("check_my_work", &serde_json::Value::Null, 131 * s)
            .is_ok()
    );
    assert!(
        engine
            .tool("check_my_work", &serde_json::Value::Null, 161 * s)
            .is_err()
    );
}

#[test]
fn an_expired_queued_call_is_never_committed() {
    // The caller gave up while the call waited in the queue: the loop
    // cannot commit it after, and a committed call cannot be given up.
    let late = Admission::default();
    assert!(late.cancel());
    assert!(!late.commit());
    let taken = Admission::default();
    assert!(taken.commit());
    assert!(!taken.cancel());
}

/// The owner, 19 Oct: installable skills go on and off a running lamp.
/// Desk tutor, packed for the Store and installed beside Dance, a core
/// skill, reaches the voice's functions at the program's next look at its
/// folder, and goes again when removed; the program is never restarted,
/// and Dance can't be removed.
#[test]
fn an_installable_skill_goes_on_and_off_a_running_lamp_with_nothing_restarted() {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("si")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    let skills = root.join("skills");
    copy(&skills_root().join("dance"), &skills.join("dance"));
    let tools = root.join("tools");
    let clock = Clock::at(root);
    let mut running = Running(
        Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args([
                "--dir".as_ref(),
                skills.as_os_str(),
                "--bus-root".as_ref(),
                root.join("bus").as_os_str(),
                "--tools-dir".as_ref(),
                tools.as_os_str(),
            ])
            .args(clock.args())
            .spawn()
            .unwrap(),
    );
    clock.started();
    let functions = || -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // Written whole (renamed into place) or removed.
            match fs::read_to_string(tools.join("tools.json")) {
                Ok(text) => {
                    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
                    return file["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|tool| tool["name"].as_str().unwrap().to_owned())
                        .collect();
                }
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(error) => panic!("no tools file: {error}"),
            }
        }
    };
    let settled = |wanted: &[&str]| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let now = functions();
            if now == wanted {
                return;
            }
            assert!(Instant::now() < deadline, "{now:?}, not {wanted:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    settled(&["dance"]);
    let command = |args: &[&OsStr]| {
        let out = Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args(args)
            .output()
            .unwrap();
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let source = root.join("src/desk-tutor");
    copy(&skills_root().join("desk-tutor"), &source);
    let (code, said, _) = command(&[
        "pack".as_ref(),
        "--out".as_ref(),
        root.as_os_str(),
        source.as_os_str(),
    ]);
    assert_eq!(code, Some(0));
    let package = root.join("desk-tutor-1.0.0.tar");
    assert_eq!(said.trim(), format!("packed {}", package.display()));
    let (code, said, error) = command(&[
        "install".as_ref(),
        "--dir".as_ref(),
        skills.as_os_str(),
        package.as_os_str(),
    ]);
    assert_eq!(code, Some(0), "{error}");
    assert!(said.starts_with("installed desk-tutor 1.0.0"), "{said}");
    clock.skip(RELOAD);
    // In folder order: Dance's, then Desk tutor's.
    settled(&["dance", "check_my_work"]);
    let (code, listed, _) = command(&["list".as_ref(), "--dir".as_ref(), skills.as_os_str()]);
    assert_eq!(code, Some(0));
    let mut lines: Vec<&str> = listed.lines().collect();
    // Last, what the skills give the model, against a session's room.
    let functions = lines.pop().unwrap_or_default();
    assert!(
        functions.starts_with(
            "The skills here give the model 2 functions (dance, check_my_work). A session takes 32"
        ),
        "{listed}"
    );
    assert!(said.contains(functions), "{said}");
    let listed: Vec<Vec<&str>> = lines
        .iter()
        .map(|line| line.split_whitespace().take(3).collect())
        .collect();
    assert_eq!(
        listed,
        [
            ["NAME", "KIND", "VERSION"],
            ["dance", "core", "1.0.0"],
            ["desk-tutor", "installable", "1.0.0"]
        ]
    );
    // A core skill stays, and says why.
    let (code, said, error) = command(&[
        "remove".as_ref(),
        "--dir".as_ref(),
        skills.as_os_str(),
        "dance".as_ref(),
    ]);
    assert_eq!((code, said.as_str()), (Some(1), ""));
    assert_eq!(
        error.trim(),
        "lamp-skills: dance is a core skill: it comes with the lamp and can't be removed"
    );
    let (code, said, error) = command(&[
        "remove".as_ref(),
        "--dir".as_ref(),
        skills.as_os_str(),
        "desk-tutor".as_ref(),
    ]);
    assert_eq!(code, Some(0), "{error}");
    assert!(said.starts_with("removed desk-tutor"), "{said}");
    clock.skip(RELOAD);
    settled(&["dance"]);
    // The same program throughout.
    assert!(running.0.try_wait().unwrap().is_none());
    let (code, _, error) = command(&["install".as_ref()]);
    assert_eq!(code, Some(2));
    assert!(error.contains("lamp-skills install [--dir DIR]"), "{error}");
}

#[test]
fn a_declared_tool_is_in_the_tools_file_and_a_call_runs_the_skills_steps() {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("st")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    fs::create_dir(root.join("skills")).unwrap();
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let mut ring = bus
        .publisher_declared("light/state", &Light::declaration(), "ring")
        .unwrap();
    let actions = UnixListener::bind(root.join("actions.sock")).unwrap();
    let (called, lights) = mpsc::channel();
    // The actions part, saying the colour it shows as the real one does.
    let shown = bus.clone();
    std::thread::spawn(move || {
        let mut shown = shown
            .publisher_declared("light/state", &Light::declaration(), "ring")
            .unwrap();
        for stream in actions.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let call: serde_json::Value = serde_json::from_str(&line).unwrap();
            if let Some(colour) = call["args"]["colour"].as_str() {
                shown
                    .publish_json(1, &Light::new(true, colour, 3).unwrap())
                    .unwrap();
            }
            (&stream)
                .write_all(b"{\"ok\":true,\"result\":{}}\n")
                .unwrap();
            let _ = called.send(line);
        }
    });
    let tools = root.join("tools");
    let clock = Clock::at(root);
    let _running = Running(
        Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args([
                "--dir".as_ref(),
                root.join("skills").as_os_str(),
                "--bus-root".as_ref(),
                root.join("bus").as_os_str(),
                "--actions-socket".as_ref(),
                root.join("actions.sock").as_os_str(),
                "--tools-dir".as_ref(),
                tools.as_os_str(),
            ])
            .args(clock.args())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !tools.join("call.sock").exists() {
        assert!(Instant::now() < deadline, "no call.sock");
        std::thread::sleep(Duration::from_millis(50));
    }
    // No skill with a tool: no tools file.
    clock.started();
    assert!(!tools.join("tools.json").exists());
    skill_with(&root.join("skills"), "tutor", TUTOR);
    clock.skip(RELOAD);
    let file = loop {
        if let Ok(text) = fs::read_to_string(tools.join("tools.json")) {
            break serde_json::from_str::<serde_json::Value>(&text).unwrap();
        }
        assert!(Instant::now() < deadline, "no tools file");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        file,
        // A skill that asks for the body is named beside the tools: a text
        // never runs it (#1673).
        serde_json::json!({"tools":[{"name":"check_my_work","description":"A test."}],
            "body":["check_my_work"]})
    );
    // The voice's call, as it asks the actions part.
    let call = |name: &str| {
        let stream = std::os::unix::net::UnixStream::connect(tools.join("call.sock")).unwrap();
        writeln!(&stream, r#"{{"op":"call","name":"{name}","args":{{}}}}"#).unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    // The light as shown just before the call, fresh.
    ring.publish_json(1, &Light::new(true, "blue", 3).unwrap())
        .unwrap();
    let asked = Instant::now();
    let answer = call("check_my_work");
    assert!(
        asked.elapsed() < Duration::from_millis(500),
        "{:?}",
        asked.elapsed()
    );
    assert_eq!(answer["ok"], true, "{answer}");
    let next = answer["result"]["next"].as_str().unwrap();
    assert!(
        next.starts_with("Say only \"Let me take a look.\""),
        "{next}"
    );
    assert!(next.ends_with("never the answer."), "{next}");
    // And the voice takes the look itself, whatever the model says (#941).
    assert_eq!(answer["result"]["look"], true, "{answer}");
    // Again while the page is still lit, as a student asks once more after
    // fixing a line (#941): told to look again, and the light left as it is.
    let again = call("check_my_work");
    assert_eq!(again["ok"], true, "{again}");
    assert_eq!(again["result"]["next"], answer["result"]["next"]);
    assert_eq!(call("nobody_has_this")["ok"], false);
    // The page's light lasts its `for`, a second.
    clock.skip(Duration::from_secs(1));
    let mut seen = Vec::new();
    while seen
        .iter()
        .filter(|event: &&SkillEvent| event.stage == SkillStage::Done)
        .count()
        < 2
        && Instant::now() < deadline
    {
        if let Some(message) = events.recv(Duration::from_millis(100)).unwrap() {
            seen.push(message.parse().unwrap());
        }
    }
    assert_eq!(
        stages(&seen),
        [
            SkillStage::Triggered,
            SkillStage::Triggered,
            SkillStage::Done,
            SkillStage::Done
        ]
    );
    assert!(seen[2].why.contains("still under way"), "{seen:?}");
    let lights: Vec<String> = lights.try_iter().collect();
    // The page lit once, and after its second given back: nothing made for
    // the second call.
    assert_eq!(lights.len(), 2, "{lights:?}");
    assert!(
        lights[0].contains("\"light_colour\"") && lights[0].contains("white"),
        "{lights:?}"
    );
    assert!(lights[1].contains("blue"), "{lights:?}");
}

/// A skill whose tool only makes things: nothing to tell the model, so a
/// call while its run is under way is still refused.
#[test]
fn a_repeat_call_with_only_made_steps_is_refused_while_its_run_is_under_way() {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("st")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    fs::create_dir(root.join("skills")).unwrap();
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut ring = bus
        .publisher_declared("light/state", &Light::declaration(), "ring")
        .unwrap();
    let actions = UnixListener::bind(root.join("actions.sock")).unwrap();
    std::thread::spawn(move || {
        for stream in actions.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            (&stream)
                .write_all(b"{\"ok\":true,\"result\":{}}\n")
                .unwrap();
        }
    });
    let tools = root.join("tools");
    let _running = Running(
        Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args([
                "--dir".as_ref(),
                root.join("skills").as_os_str(),
                "--bus-root".as_ref(),
                root.join("bus").as_os_str(),
                "--actions-socket".as_ref(),
                root.join("actions.sock").as_os_str(),
                "--tools-dir".as_ref(),
                tools.as_os_str(),
            ])
            .spawn()
            .unwrap(),
    );
    skill_with(
        &root.join("skills"),
        "glow",
        &TUTOR
            .replace("check_my_work", "glow")
            .replace("look = true\n", "")
            .replace(
                "say = \"Tutor: a hint or a question, never the answer.\"\n",
                "",
            )
            .replace("for = \"1s\"", "for = \"3s\""),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !tools.join("tools.json").exists() {
        assert!(Instant::now() < deadline, "no tools file");
        std::thread::sleep(Duration::from_millis(50));
    }
    let call = || {
        let stream = std::os::unix::net::UnixStream::connect(tools.join("call.sock")).unwrap();
        writeln!(&stream, r#"{{"op":"call","name":"glow","args":{{}}}}"#).unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    ring.publish_json(1, &Light::new(true, "blue", 3).unwrap())
        .unwrap();
    let first = call();
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["result"]["next"], "Done.");
    // A tool without a look asks the voice for none.
    assert!(first["result"].get("look").is_none(), "{first}");
    let again = call();
    assert_eq!(again["ok"], false, "{again}");
    assert_eq!(again["error"], "I'm still doing that.");
}

/// A repeat call while the run's light is still on holds its pose again,
/// so each look comes with the head down over the page (#1051, Ten's P2):
/// a 3 s pose, a call, 5 s later another, and the pose is said again.
#[test]
fn a_repeat_tool_call_holds_the_runs_pose_again_and_only_that() {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("sm")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    fs::create_dir(root.join("skills")).unwrap();
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut ring = bus
        .publisher_declared("light/state", &Light::declaration(), "ring")
        .unwrap();
    let actions = UnixListener::bind(root.join("actions.sock")).unwrap();
    let (called, lights) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in actions.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            // Kept before it is answered: a call answered was seen.
            let _ = called.send(line);
            (&stream)
                .write_all(b"{\"ok\":true,\"result\":{}}\n")
                .unwrap();
        }
    });
    let mood = UnixDatagram::bind(root.join("mood.sock")).unwrap();
    mood.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let tools = root.join("tools");
    let clock = Clock::at(root);
    let _running = Running(
        Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args([
                "--dir".as_ref(),
                root.join("skills").as_os_str(),
                "--bus-root".as_ref(),
                root.join("bus").as_os_str(),
                "--mood-socket".as_ref(),
                root.join("mood.sock").as_os_str(),
                "--actions-socket".as_ref(),
                root.join("actions.sock").as_os_str(),
                "--tools-dir".as_ref(),
                tools.as_os_str(),
            ])
            .args(clock.args())
            .spawn()
            .unwrap(),
    );
    skill_with(
        &root.join("skills"),
        "tutor",
        &TUTOR.replace(
            "light = { colour = \"white\", for = \"1s\" }",
            "light = { colour = \"white\", for = \"10s\" }\nbody = { mood = \"reading\", for = \"3s\" }",
        ),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !tools.join("tools.json").exists() {
        assert!(Instant::now() < deadline, "no tools file");
        std::thread::sleep(Duration::from_millis(50));
    }
    let call = || {
        let stream = std::os::unix::net::UnixStream::connect(tools.join("call.sock")).unwrap();
        writeln!(
            &stream,
            r#"{{"op":"call","name":"check_my_work","args":{{}}}}"#
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    // Poses said over `window` of the program's clock, from now: a second
    // on at a time, what it says in it waited for.
    let poses = |window: Duration| {
        let until = clock.now() + window;
        let mut buffer = [0u8; 256];
        let mut count = 0;
        while clock.now() < until {
            clock.skip_to((clock.now() + Duration::from_secs(1)).min(until));
            if let Ok(length) = mood.recv(&mut buffer) {
                assert_eq!(&buffer[..length], br#"{"level":0,"mood":"reading"}"#);
                count += 1;
            }
        }
        count
    };
    ring.publish_json(1, &Light::new(true, "blue", 3).unwrap())
        .unwrap();
    // Whether the pose was already said when the call was answered.
    let posed = || {
        mood.set_nonblocking(true).unwrap();
        let mut buffer = [0u8; 256];
        let said = mood.recv(&mut buffer).is_ok();
        mood.set_nonblocking(false).unwrap();
        said
    };
    assert_eq!(call()["ok"], true);
    // The head sent down and the page lit before the answer (#1146): the
    // voice asks the eyes as the answer's first audio plays, with the head
    // already moving, so the eyes wait for it to settle over the page.
    assert!(posed(), "the pose came after the answer");
    let lit = lights.try_recv().expect("the light came after the answer");
    assert!(lit.contains("white"), "{lit}");
    let first = 1 + poses(Duration::from_millis(3_500));
    // Let go: nothing said between the pose's end and the second call.
    let quiet = poses(Duration::from_millis(1_500));
    let again = call();
    assert_eq!(again["ok"], true, "{again}");
    assert!(posed(), "the pose again came after the answer");
    let second = 1 + poses(Duration::from_millis(3_500));
    assert_eq!((first, quiet, second), (3, 0, 3));
    // The light was made once, taken above: the second call made only the
    // pose.
    let lights: Vec<String> = lights.try_iter().collect();
    assert!(lights.is_empty(), "{lights:?}");
}

/// A hello on every face that appears after a second without one.
fn hello_after_a_second() -> String {
    AT_ONCE
        .replace("light = { colour = \"warm\" }\n", "")
        .replace("near = 0.14", "absent_for = \"1s\"")
}

/// The program with `LAMP_SKILLS_ASK=policy` on a bus of its own in `root`,
/// once its hello is loaded, and the voice's say service stood in for: each
/// line it is asked to say, and when.
fn asking_the_policy_in(
    root: &Path,
    clock: &Clock,
) -> (Running, Bus, mpsc::Receiver<(String, Instant)>) {
    fs::create_dir(root.join("skills")).unwrap();
    let bus = Bus::new(root.join("bus")).unwrap();
    let voice = UnixListener::bind(root.join("say.sock")).unwrap();
    let (heard, asked) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in voice.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let _ = heard.send((line, Instant::now()));
            (&stream).write_all(b"{\"outcome\":\"spoken\"}\n").unwrap();
        }
    });
    // On the host's clock the hello is there before the start, so it loads
    // at once: found by the reload it would cost `RELOAD`, two real seconds
    // (#1128). On a clock the test moves it is put in after, as before.
    let on_the_host = clock.socket.is_none();
    if on_the_host {
        skill_with(&root.join("skills"), "hello", &hello_after_a_second());
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    command
        .args([
            "--dir".as_ref(),
            root.join("skills").as_os_str(),
            "--bus-root".as_ref(),
            root.join("bus").as_os_str(),
            "--gaze-socket".as_ref(),
            root.join("gaze.sock").as_os_str(),
            "--say-socket".as_ref(),
            root.join("say.sock").as_os_str(),
            "--actions-socket".as_ref(),
            root.join("actions.sock").as_os_str(),
        ])
        .args(clock.args())
        .env("LAMP_SKILLS_ASK", "policy");
    let running = Running(command.spawn().unwrap());
    clock.started();
    if !on_the_host {
        skill_with(&root.join("skills"), "hello", &hello_after_a_second());
        clock.skip(RELOAD);
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while bus.subscribers(Faces::TOPIC).unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the skill was never loaded");
        std::thread::sleep(Duration::from_millis(50));
    }
    (running, bus, asked)
}

/// Nobody for a little over a second of the program's clock, then a face,
/// and `leave`: gone again at once. When the face was published: just
/// before, for a lower bound (the program may ask before the publish
/// returns), and just after, for an upper one.
fn a_face_appears(
    eyes: &mut lamp_bus::Publisher,
    clock: &Clock,
    leave: bool,
) -> (Instant, Instant) {
    eyes.publish_json(1, &serde_json::json!({"faces": []}))
        .unwrap();
    clock.skip(Duration::from_millis(1_300));
    let before = Instant::now();
    eyes.publish_json(1, &serde_json::from_str::<serde_json::Value>(FACE).unwrap())
        .unwrap();
    let after = Instant::now();
    if leave {
        eyes.publish_json(1, &serde_json::json!({"faces": []}))
            .unwrap();
    }
    (before, after)
}

/// The next `answered` event, within `deadline`.
fn next_answer(events: &mut lamp_bus::Subscriber, deadline: Instant) -> SkillEvent {
    while Instant::now() < deadline {
        if let Some(message) = events.recv(Duration::from_millis(100)).unwrap() {
            let event: SkillEvent = message.parse().unwrap();
            if event.stage == SkillStage::Answered {
                return event;
            }
        }
    }
    panic!("no answer in time");
}

/// With `LAMP_SKILLS_ASK=policy` a skill asks on `skills/ask` (#958's
/// `SkillAsk`) and acts only on a `say_now` not in shadow that names its
/// ask while its trigger still holds: a hello with its minutes away and
/// seconds there (after an answer to another ask, passed over), then a
/// `wait`, a `say_now` in shadow, one without `shadow`, one too late and
/// one after the face has gone, each left undone.
#[test]
fn asking_the_policy_a_skill_acts_only_on_a_say_now_that_names_its_ask() {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("sp")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    // The Policy stood in for, an ask at a time: its choice, `shadow`,
    // how long it takes, and whether it first answers another ask.
    let script: [(&str, Option<bool>, u64, bool); 6] = [
        ("say_now", Some(false), 0, true),
        ("wait", Some(false), 0, false),
        ("say_now", Some(true), 0, false),
        ("say_now", None, 0, false),
        ("say_now", Some(false), 2_800, false),
        ("say_now", Some(false), 300, false),
    ];
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let (questions, put) = mpsc::channel::<serde_json::Value>();
    let clock = Clock::at(root);
    let (policy, takes) = (bus.clone(), clock.clone());
    // The last face leaves at once: told once its leave is published, so
    // the stand-in's skip takes the leave in before the answer comes.
    let (gone, left) = mpsc::channel::<()>();
    let mut asks = policy.subscribe("skills/ask", "policy").unwrap();
    std::thread::spawn(move || {
        let mut decisions = policy
            .publisher_declared(
                "policy/decisions",
                &lamp_msgs::Decision::declaration(),
                "policy",
            )
            .unwrap();
        for (cycle, (choice, shadow, takes_ms, other_first)) in script.into_iter().enumerate() {
            let message = loop {
                if let Ok(Some(message)) = asks.recv(Duration::from_millis(100)) {
                    break message;
                }
            };
            let ask: serde_json::Value = serde_json::from_slice(&message.payload).unwrap();
            let _ = questions.send(ask.clone());
            let number = ask["ask"].as_u64().unwrap();
            let mut decision = serde_json::json!({"about": "hello", "choice": choice,
                "probability": 0.9, "answers": {}, "by": "rule", "took_ms": 3});
            if let Some(shadow) = shadow {
                decision["shadow"] = shadow.into();
            }
            if other_first {
                decision["ask"] = (number + 1).into();
                decisions.publish_json(1, &decision).unwrap();
            }
            if cycle == 5 {
                left.recv().unwrap();
            }
            // Its time is on the program's clock.
            if takes_ms > 0 {
                takes.skip(Duration::from_millis(takes_ms));
            }
            decision["ask"] = number.into();
            decisions.publish_json(1, &decision).unwrap();
        }
    });
    let (_running, bus, said) = asking_the_policy_in(root, &clock);
    let mut eyes = bus
        .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut answered = Vec::new();
    let mut face_to_say = None;
    for cycle in 0..script.len() {
        let (_, face_at) = a_face_appears(&mut eyes, &clock, cycle == 5);
        if cycle == 5 {
            gone.send(()).unwrap();
        }
        answered.push(next_answer(&mut events, deadline));
        if cycle == 0 {
            let (_, said_at) = said.recv_timeout(Duration::from_secs(2)).unwrap();
            face_to_say = Some(said_at - face_at);
        }
    }
    let face_to_say = face_to_say.unwrap();
    println!("face published to say asked, the Policy answering at once: {face_to_say:?}");
    // The Policy's own time is nothing here: what is left is the bus and
    // the program, well under its old 250 ms wait.
    assert!(face_to_say < Duration::from_millis(100), "{face_to_say:?}");
    let put: Vec<serde_json::Value> = put.try_iter().collect();
    assert_eq!(put.len(), script.len(), "{put:?}");
    assert_eq!(put[0]["skill"], "hello");
    assert_eq!(put[0]["act"], "hello");
    assert!(put[0]["away_min"].is_u64() && put[0]["there_s"].is_u64());
    let numbers: Vec<u64> = put.iter().map(|ask| ask["ask"].as_u64().unwrap()).collect();
    assert!(numbers[0] > 0 && numbers.windows(2).all(|pair| pair[0] < pair[1]));
    let allowed: Vec<Option<bool>> = answered.iter().map(|event| event.allowed).collect();
    assert_eq!(
        allowed,
        [
            Some(true),
            Some(false),
            Some(false),
            Some(false),
            Some(false),
            Some(false)
        ],
        "{answered:?}"
    );
    assert!(answered[1].why.contains("wait"), "{answered:?}");
    assert!(answered[2].why.contains("in shadow"), "{answered:?}");
    assert!(answered[3].why.contains("in shadow"), "{answered:?}");
    assert!(answered[4].why.contains("did not answer"), "{answered:?}");
    assert!(answered[5].why.contains("no longer holds"), "{answered:?}");
    // Only the first say_now was said, the late one passed over too.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(said.try_iter().count(), 0);
}

/// On the lamp's own clock, without `--clock-control`: a Policy that
/// answers only once it has been given up is given up at its 2.5 s, not
/// before, and its late `say_now` is passed over.
#[test]
fn asking_the_policy_on_the_hosts_clock_gives_up_after_its_wait() {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("sh")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let mut asks = bus.subscribe("skills/ask", "policy").unwrap();
    let policy = bus.clone();
    // The give-up seen: the Policy's late answer is sent then, not on a
    // wait of its own.
    let (given_up, late) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut decisions = policy
            .publisher_declared(
                "policy/decisions",
                &lamp_msgs::Decision::declaration(),
                "policy",
            )
            .unwrap();
        let message = loop {
            if let Ok(Some(message)) = asks.recv(Duration::from_millis(100)) {
                break message;
            }
        };
        let ask: serde_json::Value = serde_json::from_slice(&message.payload).unwrap();
        if late.recv().is_err() {
            return;
        }
        decisions
            .publish_json(
                1,
                &serde_json::json!({"about": "hello", "choice": "say_now", "shadow": false,
                    "probability": 0.9, "answers": {}, "by": "rule", "took_ms": 3,
                    "ask": ask["ask"]}),
            )
            .unwrap();
    });
    let clock = Clock::host();
    let (_running, bus, said) = asking_the_policy_in(root, &clock);
    let mut eyes = bus
        .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
        .unwrap();
    let (face_at, _) = a_face_appears(&mut eyes, &clock, false);
    let answer = next_answer(&mut events, Instant::now() + Duration::from_secs(10));
    let waited = face_at.elapsed();
    assert_eq!(answer.allowed, Some(false), "{answer:?}");
    assert!(answer.why.contains("did not answer"), "{answer:?}");
    assert!(waited >= Duration::from_millis(2_500), "{waited:?}");
    // The late say_now, sent now and naming the given-up ask, is passed
    // over: no answer to it within 400 ms of the host's clock, and nothing
    // said. Were it acted on, it would be answered and said at once.
    given_up.send(()).unwrap();
    let window = Instant::now() + Duration::from_millis(400);
    while Instant::now() < window {
        if let Some(message) = events.recv(Duration::from_millis(50)).unwrap() {
            let event: SkillEvent = message.parse().unwrap();
            assert!(event.stage != SkillStage::Answered, "{event:?}");
        }
    }
    assert_eq!(said.try_iter().count(), 0);
}

/// With nobody on `skills/ask` (the Policy down, or without `--gate`) the
/// run is refused at once, not after the Policy's 2.5 s.
#[test]
fn asking_the_policy_with_nobody_to_answer_is_no_at_once() {
    let dir = tempfile::Builder::new()
        .prefix("sn")
        .tempdir_in("/tmp")
        .unwrap();
    let clock = Clock::at(dir.path());
    let (_running, bus, said) = asking_the_policy_in(dir.path(), &clock);
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let mut eyes = bus
        .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
        .unwrap();
    let (_, face_at) = a_face_appears(&mut eyes, &clock, false);
    let answer = next_answer(&mut events, Instant::now() + Duration::from_secs(5));
    assert!(face_at.elapsed() < Duration::from_secs(1), "{answer:?}");
    assert_eq!(answer.allowed, Some(false));
    assert!(answer.why.contains("nobody is subscribed"), "{answer:?}");
    assert_eq!(said.try_iter().count(), 0);
}

/// A skill that dims until its trigger ends, the light blue at 3 and the
/// actions part stood in for, its ask out to the Policy; with `moved`, the
/// skill reloaded meanwhile with its trigger moved to another topic that
/// also holds. Then the Policy says `say_now`, and, after a while, the old
/// trigger stops holding. Its answer, the `done` if any (within 3 s, or
/// 300 ms where none is expected), and the
/// actions part's calls.
fn a_held_dim_asked_of_the_policy(moved: bool) -> (SkillEvent, Option<SkillEvent>, Vec<String>) {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("sr")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let mut asks = bus.subscribe("skills/ask", "policy").unwrap();
    // The actions part stood in for: each call, the light it shows said.
    let actions = UnixListener::bind(root.join("actions.sock")).unwrap();
    let (called, calls) = mpsc::channel::<String>();
    let ring = bus.clone();
    std::thread::spawn(move || {
        let mut shown = ring
            .publisher_declared("light/state", &Light::declaration(), "ring")
            .unwrap();
        let mut light = Light::new(true, "blue", 3).unwrap();
        for stream in actions.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            if line.contains("light_dimmer") {
                light.level -= 1;
            } else if line.contains("light_brighter") {
                light.level += 1;
            }
            shown.publish_json(1, &light).unwrap();
            (&stream)
                .write_all(b"{\"ok\":true,\"result\":{}}\n")
                .unwrap();
            let _ = called.send(line);
        }
    });
    // The Policy stood in for: the ask is told, and answered `say_now`
    // once the test says so.
    let (asked, put) = mpsc::channel::<u64>();
    let (answer, go) = mpsc::channel::<()>();
    let policy = bus.clone();
    std::thread::spawn(move || {
        let mut decisions = policy
            .publisher_declared(
                "policy/decisions",
                &lamp_msgs::Decision::declaration(),
                "policy",
            )
            .unwrap();
        let message = loop {
            if let Ok(Some(message)) = asks.recv(Duration::from_millis(100)) {
                break message;
            }
        };
        let ask: serde_json::Value = serde_json::from_slice(&message.payload).unwrap();
        asked.send(ask["ask"].as_u64().unwrap()).unwrap();
        go.recv().unwrap();
        decisions
            .publish_json(
                1,
                &serde_json::json!({"about": "unprompted", "choice": "say_now",
                    "shadow": false, "probability": 0.9, "answers": {}, "by": "rule",
                    "took_ms": 3, "ask": ask["ask"]}),
            )
            .unwrap();
    });
    let clock = Clock::at(root);
    let (_running, bus, _said) = asking_the_policy_in(root, &clock);
    let air = lamp_bus::Declaration::new("test/Air", 1, lamp_bus::Kind::Latest);
    let mut state = bus.publisher_declared("air/state", &air, "air").unwrap();
    let mut indoor = bus.publisher_declared("air/indoor", &air, "air").unwrap();
    let stuffy = "[trigger]\ntopic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\ncooldown = \"10m\"\n\
        [ask]\npolicy = \"nudge\"\n[[do]]\nlight = { dim = 1, until = \"ends\" }\n";
    let skills = root.join("skills");
    skill_with(&skills, "stuffy", stuffy);
    clock.skip(RELOAD);
    let loaded = |topic: &str| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while bus.subscribers(topic).unwrap().is_empty() {
            assert!(Instant::now() < deadline, "{topic} was never subscribed");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    loaded("air/state");
    // The light as shown just before the trigger, fresh.
    bus.publisher_declared("light/state", &Light::declaration(), "ring")
        .unwrap()
        .publish_json(1, &Light::new(true, "blue", 3).unwrap())
        .unwrap();
    let high = serde_json::json!({"co2_ppm": 1300});
    state.publish_json(1, &high).unwrap();
    put.recv_timeout(Duration::from_secs(10)).unwrap();
    if moved {
        // Its trigger moved to air/indoor, which holds too.
        skill_with(
            &skills,
            "stuffy",
            &stuffy.replace("air/state", "air/indoor"),
        );
        clock.skip(RELOAD);
        loaded("air/indoor");
        indoor.publish_json(1, &high).unwrap();
    }
    // What was published taken in, within the ask's 2.5 s.
    clock.skip(Duration::ZERO);
    answer.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let answered = next_answer(&mut events, deadline);
    // The dim made, if it is, before the trigger stops holding.
    let dimmed = calls.recv_timeout(Duration::from_millis(if moved { 300 } else { 5_000 }));
    state
        .publish_json(1, &serde_json::json!({"co2_ppm": 900}))
        .unwrap();
    clock.skip(Duration::ZERO);
    let mut done = None;
    let until = Instant::now() + Duration::from_millis(if moved { 300 } else { 3_000 });
    while done.is_none() && Instant::now() < until {
        if let Some(message) = events.recv(Duration::from_millis(100)).unwrap() {
            let event: SkillEvent = message.parse().unwrap();
            if event.stage == SkillStage::Done && event.skill == "stuffy" {
                done = Some(event);
            }
        }
    }
    let made = dimmed.into_iter().chain(calls.try_iter()).collect();
    (answered, done, made)
}

/// A dim held until its trigger ends, its ask out to the Policy, and the
/// skill reloaded with its trigger moved to another topic that also holds
/// (#1309's post-merge P3): the reload ends the hold before the run began,
/// so the Policy's `say_now` then does nothing, rather than dim a light
/// whose end would never come.
#[test]
fn a_say_now_after_a_reload_moved_a_held_trigger_does_nothing() {
    let (answered, done, calls) = a_held_dim_asked_of_the_policy(true);
    assert_eq!(answered.skill, "stuffy", "{answered:?}");
    assert_eq!(answered.allowed, Some(false), "{answered:?}");
    assert!(answered.why.contains("no longer holds"), "{answered:?}");
    assert!(done.is_none(), "{done:?}");
    assert!(calls.is_empty(), "{calls:?}");
}

/// Its control (Fourth on #1319): with no reload, the same `say_now` dims
/// the light, and the dim is given back when the trigger stops holding.
#[test]
fn a_say_now_with_its_trigger_still_held_dims_until_it_ends() {
    let (answered, done, calls) = a_held_dim_asked_of_the_policy(false);
    assert_eq!(answered.allowed, Some(true), "{answered:?}");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].contains("light_dimmer"), "{calls:?}");
    assert!(calls[1].contains("light_brighter"), "{calls:?}");
    let done = done.expect("no done after the trigger ended");
    assert!(
        done.why.contains("held until its trigger ended"),
        "{}",
        done.why
    );
}

/// A skill that is not a hello asks the act its `[ask] policy` names, news,
/// a reminder or anything else as unprompted, without a hello's minutes and
/// seconds.
#[test]
fn asking_the_policy_a_skill_that_is_not_a_hello_asks_its_act() {
    let dir = tempfile::tempdir().unwrap();
    for (policy, act) in [
        ("news", "news"),
        ("reminder", "reminder"),
        ("nudge", "unprompted"),
    ] {
        let toml =
            hello_after_a_second().replace("policy = \"hello\"", &format!("policy = \"{policy}\""));
        let skill = skill_with(dir.path(), policy, &toml);
        let mut engine = Engine::new(vec![load(&skill).unwrap()], false).asking_the_policy();
        assert!(
            engine
                .observe(Faces::TOPIC, 0, NOBODY.as_bytes(), None)
                .is_empty()
        );
        let runs = engine.observe(Faces::TOPIC, 2_000_000, FACE.as_bytes(), None);
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].ask,
            Some(serde_json::json!({"skill": policy, "act": act}))
        );
    }
}

#[test]
fn a_skills_own_move_loads_as_the_body_takes_it_and_a_bad_one_is_refused_with_why() {
    let toml = AT_ONCE.replace(
        "body = { look_at = \"face\" }",
        "body = { move = \"sway\" }",
    );
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "hello", &toml);
    fs::create_dir(skill.join("moves")).unwrap();
    fs::write(skill.join("moves/sway.json"), sway("sway", 60)).unwrap();
    let runs = runs_at(
        &skill,
        &[(0, Faces::TOPIC, NOBODY), (1, Faces::TOPIC, FACE)],
    );
    let calls = &runs[0].1;
    let Some(Call::Move(clip)) = calls.first() else {
        panic!("{runs:?}");
    };
    assert_eq!(clip.name, "sway");
    assert_eq!(clip.frames.len(), 41);
    assert_eq!(
        calls[0].named(),
        ("body.move", serde_json::json!({ "move": "sway" }))
    );
    // What the program sends the mood socket is a move the body plays.
    let datagram = serde_json::json!({ "move": clip }).to_string();
    let played = lamp_body::moves::Sent::default()
        .take(datagram.as_bytes())
        .unwrap()
        .unwrap();
    assert_eq!((played.name, played.frames), ("sway", &clip.frames[..]));
    // What the body would refuse is refused here, and why.
    for (text, why) in [
        (sway("sway", 201), "farther than 200"),
        (
            sway("sway", 60).replace("\"step_ms\":50", "\"step_ms\":20"),
            "not from 40 to 1000",
        ),
        (
            sway("sway", 60).replace("\"step_ms\":50", "\"step_ms\":300"),
            "not 1500 to 10000",
        ),
        (
            sway("sway", 60).replace("\"step_ms\"", "\"speed\":2,\"step_ms\""),
            "not a move",
        ),
        (sway("nod", 60), "names \"nod\", not \"sway\""),
    ] {
        fs::write(skill.join("moves/sway.json"), &text).unwrap();
        let error = load(&skill).unwrap_err();
        assert!(
            error.contains("moves/sway.json") && error.contains(why),
            "{why}: {error}"
        );
    }
    // Looked at before it is read: too long, or not a regular file (a pipe
    // there would stall every skill's load).
    let long = sway("sway", 60).replace(
        "\"step_ms\"",
        &format!("{}\"step_ms\"", " ".repeat(300 * 1024)),
    );
    fs::write(skill.join("moves/sway.json"), long).unwrap();
    assert!(
        load(&skill)
            .unwrap_err()
            .contains("longer than 262144 bytes")
    );
    fs::remove_file(skill.join("moves/sway.json")).unwrap();
    fs::create_dir(skill.join("moves/sway.json")).unwrap();
    assert!(
        load(&skill)
            .unwrap_err()
            .contains("moves/sway.json: not a file")
    );
    // A move in the body's own name, and a file that is not there.
    fs::write(skill.join("moves/nod.json"), sway("nod", 60)).unwrap();
    for (step, why) in [
        (
            "body = { move = \"nod\" }",
            "one of the body's own gestures",
        ),
        ("body = { move = \"gone\" }", "moves/gone.json"),
        ("body = { move = \"../sway\" }", "not a file name in moves/"),
    ] {
        let toml = AT_ONCE.replace("body = { look_at = \"face\" }", step);
        fs::write(skill.join("skill.toml"), granted(&toml)).unwrap();
        let error = load(&skill).unwrap_err();
        assert!(error.contains(why), "{step}: {error}");
    }
}

/// A 2 s move of the head and back, 50 ms a step: the body's file format.
fn sway(name: &str, peak: i16) -> String {
    let mut frames = vec![[0i16; 5]];
    for step in 1..40i16 {
        let share = if step <= 20 { step } else { 40 - step };
        frames.push([0, peak * share / 20, 0, 0, 0]);
    }
    frames.push([0; 5]);
    serde_json::json!({ "name": name, "step_ms": 50, "frames": frames }).to_string()
}

/// Dance's shape (#999): a tool with a choice and a flag, its clip after the
/// model's line.
const JIG: &str = r#"
needs = ["voice", "body", "light"]
tool = "jig"
[choice]
name = "style"
about = "The mood."
one_of = ["calm", "jazz"]
[flag]
name = "long"
about = "Twice."
[[do]]
say = "Say you will."
[[do]]
after = "say"
light = { colour = "warm" }
[[do]]
body = { gesture = { calm = ["dance_chill", "dance_waltz"], jazz = ["dance_jazz"] }, again_if = "long" }
"#;

#[test]
fn a_tools_choice_and_flag_are_told_to_the_model_and_checked_on_a_call() {
    let dir = tempfile::tempdir().unwrap();
    let skill = load(&skill_with(dir.path(), "jig", JIG)).unwrap();
    assert_eq!(
        tools_file(std::slice::from_ref(&skill)),
        serde_json::json!({"tools": [{
            "name": "jig",
            "description": "A test.",
            "parameters": {"type": "object", "properties": {
                "style": {"type": "string", "enum": ["calm", "jazz"], "description": "The mood."},
                "long": {"type": "boolean", "description": "Twice."},
            }},
        }], "body": ["jig"]})
    );
    let mut engine = Engine::new(vec![skill], false);
    let mut gestures = |args: serde_json::Value| -> Vec<String> {
        engine
            .tool("jig", &args, 7)
            .unwrap()
            .calls
            .into_iter()
            .filter_map(|call| match call {
                Call::Gesture(name) => Some(name),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        gestures(serde_json::json!({"style": "jazz"})),
        ["dance_jazz"]
    );
    assert_eq!(
        gestures(serde_json::json!({"style": "jazz", "long": true})),
        ["dance_jazz", "dance_jazz"]
    );
    assert!(
        ["dance_chill", "dance_waltz"]
            .contains(&gestures(serde_json::json!({"style": "calm"}))[0].as_str())
    );
    // None given: any of the three.
    assert_eq!(gestures(serde_json::Value::Null).len(), 1);
    let calls = engine.tool("jig", &serde_json::json!({}), 7).unwrap().calls;
    assert_eq!(calls[0], Call::Say("Say you will.".into()));
    assert_eq!(calls[1], Call::AfterSay);
    for (args, why) in [
        (serde_json::json!({"style": "hype"}), "one of"),
        (serde_json::json!({"long": "yes"}), "true or false"),
        (serde_json::json!({"tempo": 3}), "not an argument"),
        (serde_json::json!([1]), "an object"),
    ] {
        let error = engine.tool("jig", &args, 7).unwrap_err();
        assert!(error.contains(why), "{args}: {error}");
    }
}

#[test]
fn the_loader_refuses_a_clip_it_does_not_have_and_arguments_it_cannot_use() {
    let dir = tempfile::tempdir().unwrap();
    for (from, to, why) in [
        (
            "\"dance_jazz\"]",
            "\"moonwalk\"]",
            "not one of the body's clips",
        ),
        (
            "\"dance_jazz\"]",
            "\"idle\"]",
            "not one of the body's clips",
        ),
        (", jazz = [\"dance_jazz\"]", "", "for each of"),
        (
            "again_if = \"long\"",
            "again_if = \"twice\"",
            "names the skill's flag",
        ),
        ("after = \"say\"", "after = \"look\"", "only \"say\""),
        ("name = \"long\"", "name = \"style\"", "different names"),
        (
            "one_of = [\"calm\", \"jazz\"]",
            "one_of = []",
            "one_of is 1 to 16",
        ),
        (
            "tool = \"jig\"",
            "[trigger]\ntopic = \"humans/faces/tracked\"\n[ask]\npolicy = \"x\"",
            "argument of the skill's tool",
        ),
    ] {
        assert!(JIG.contains(from), "{from}");
        let toml = JIG.replace(from, to);
        let error = load(&skill_with(
            &dir.path().join(why.replace([' ', '"', '\''], "_")),
            "jig",
            &toml,
        ))
        .unwrap_err();
        assert!(error.contains(why), "{to}: {error}");
    }
    // Every V1 dance, and the six gestures, are the body's.
    for clip in [
        "dance_chill",
        "dance_groove",
        "dance_hype",
        "dance_waltz",
        "dance_jazz",
        "nod",
        "stretching",
    ] {
        assert!(lamp_skills::gesture_known(clip), "{clip}");
    }
}

/// The clip a short test plays: the body's nod, about 3 s.
const NODS: &str = r#"
tool = "nods"
[flag]
name = "long"
about = "Twice."
[[do]]
say = "Say you will."
[[do]]
after = "say"
light = { colour = "warm" }
[[do]]
body = { gesture = "nod", again_if = "long" }
"#;

/// The body's rule for a gesture on its mood socket
/// (`crates/body/src/body.rs`, `run_body`): at each step it reads what came
/// since the step before, once, takes a gesture only while none plays, and
/// lets a clip go at the first step that reaches its length, after that
/// step's read. A gesture that comes while one plays is dropped.
#[derive(Default)]
struct StandInBody {
    /// Since when the clip plays, its length and name.
    playing: Option<(u64, u64, String)>,
}

/// What the stand-in body says on `body/gesture` of the clips it takes.
#[derive(Clone, Copy, PartialEq)]
enum Says {
    /// `began`, then `over` when it has played, or at once when a hand
    /// pushes it (`body/hand`), as lamp-body does.
    BeganOver,
    /// `dropped`, and plays nothing: paused, or a hand holds it.
    Dropped,
    /// `began`, and nothing when it ends: a body without the topic.
    NoOver,
    /// `began`, then, a step later, `dropped`: ended for a higher ask
    /// (#1239, `ended: by conversation`), as lamp-body says it (#1230).
    EndedBy,
    /// `dropped` as [`Says::Dropped`], naming the asker, why and who holds
    /// the body, as lamp-body says it under `--arbitration on` (#940).
    Busy,
    /// As [`Says::BeganOver`], with another skill's `nod` said dropped
    /// first, which is not the run's.
    AnothersFirst,
    /// As [`Says::BeganOver`] for the first, and [`Says::Busy`] for the
    /// second: the run's last clip dropped.
    LastBusy,
}

impl StandInBody {
    /// One step at `now_us`, with the gesture read in it: whether it was
    /// taken (and so began), and the clip that ended in it, if one did.
    fn step(&mut self, now_us: u64, gesture: Option<&str>) -> (Option<bool>, Option<String>) {
        let taken = gesture.map(|name| {
            let free = self.playing.is_none();
            if free {
                self.playing = Some((now_us, clip_us(name).unwrap(), name.to_owned()));
            }
            free
        });
        let mut ended = None;
        if let Some((since, length, name)) = &self.playing
            && now_us - since >= *length
        {
            ended = Some(name.clone());
            self.playing = None;
        }
        (taken, ended)
    }

    /// Steps every `BODY_STEP_US` on `mood`, as the body does, until the
    /// socket is gone, on the program's `clock`, saying on `bus`'s
    /// `body/gesture` what `says`: each clip begun, and when, on `played`;
    /// each gesture taken (or, with [`Says::Dropped`], said dropped) on
    /// `took`; each dropped as another played on `dropped`.
    fn run(
        mood: UnixDatagram,
        bus: &Bus,
        says: Says,
        clock: Clock,
        took: mpsc::Sender<Instant>,
        played: mpsc::Sender<(Instant, String)>,
        dropped: mpsc::Sender<(Instant, String)>,
    ) {
        mood.set_nonblocking(true).unwrap();
        let mut gestures = bus
            .publisher_declared(BodyGesture::TOPIC, &BodyGesture::declaration(), "body")
            .unwrap();
        let bus = bus.clone();
        std::thread::spawn(move || {
            let mut body = Self::default();
            let began = clock.now();
            let mut next = Instant::now();
            let mut buffer = [0u8; 512];
            // Each gesture said, with whose it was and who holds the body
            // for a drop told under arbitration.
            let mut say = |name: &str, state, told: Option<(Option<String>, &str)>| {
                let (source, by) = told.unzip();
                let _ = gestures.publish_json(
                    1,
                    &BodyGesture {
                        name: name.to_owned(),
                        state,
                        source: source.flatten(),
                        why: by.map(|_| "dropped: busy, skill".to_owned()),
                        by: by.map(str::to_owned),
                    },
                );
            };
            let mut heard = 0;
            loop {
                let mut asked = None;
                loop {
                    match mood.recv(&mut buffer) {
                        Ok(length) => {
                            asked = Some(String::from_utf8_lossy(&buffer[..length]).into_owned());
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => return,
                    }
                }
                let now = clock.now();
                let ask = asked
                    .as_deref()
                    .map(|text| serde_json::from_str::<serde_json::Value>(text).unwrap());
                let name = ask
                    .as_ref()
                    .map(|ask| ask["gesture"].as_str().unwrap().to_owned());
                let source = ask
                    .as_ref()
                    .and_then(|ask| ask["source"].as_str().map(str::to_owned));
                heard += usize::from(name.is_some());
                let busy = says == Says::Busy || (says == Says::LastBusy && heard == 2);
                let now_us = u64::try_from((now - began).as_micros()).unwrap();
                if says == Says::Dropped || busy {
                    if let Some(name) = name {
                        let told = busy.then_some((source, "skill:stretch-break"));
                        say(&name, GestureState::Dropped, told);
                        let _ = took.send(now);
                    }
                } else {
                    if says == Says::AnothersFirst
                        && let Some(name) = &name
                    {
                        let other = Some((Some("skill:other".to_owned()), "skill:nods"));
                        say(name, GestureState::Dropped, other);
                    }
                    // A hand on it: the clip is let go, and said over.
                    let pushed = bus
                        .latest(Hand::TOPIC)
                        .ok()
                        .flatten()
                        .and_then(|message| message.parse::<Hand>().ok())
                        .is_some_and(|hand| hand.pushed);
                    let began_over =
                        matches!(says, Says::BeganOver | Says::AnothersFirst | Says::LastBusy);
                    if pushed
                        && began_over
                        && let Some((_, _, name)) = body.playing.take()
                    {
                        say(&name, GestureState::Over, None);
                    }
                    if says == Says::EndedBy
                        && let Some((since, _, playing)) = &body.playing
                        && now_us > *since
                    {
                        say(playing, GestureState::Dropped, None);
                        body.playing = None;
                    }
                    let (taken, ended) = body.step(now_us, name.as_deref());
                    match (taken, asked, name) {
                        (Some(true), Some(text), Some(name)) => {
                            say(&name, GestureState::Began, None);
                            let _ = took.send(now);
                            if played.send((now, text)).is_err() {
                                return;
                            }
                        }
                        (Some(false), Some(text), _) => {
                            let _ = dropped.send((now, text));
                        }
                        _ => {}
                    }
                    if let Some(name) = ended
                        && began_over
                    {
                        say(&name, GestureState::Over, None);
                    }
                }
                next += Duration::from_micros(BODY_STEP_US);
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        });
    }
}

#[test]
fn the_body_drops_a_clip_sent_at_the_first_ones_length_and_takes_one_sent_a_few_steps_later() {
    // A long dance sends a second clip after the first (#999). In time of
    // the body's own steps, at every phase of them against the sends.
    for name in [
        "nod",
        "dance_chill",
        "dance_waltz",
        "dance_groove",
        "dance_hype",
        "dance_jazz",
    ] {
        let length = clip_us(name).unwrap();
        for phase_us in (0..BODY_STEP_US).step_by(1_000) {
            let taken = |second_us: u64| -> Vec<bool> {
                let mut body = StandInBody::default();
                let sends = [0, second_us];
                let mut sent = 0;
                let mut taken = Vec::new();
                let mut now_us = phase_us;
                while now_us < 3 * length {
                    let mut asked = None;
                    while sent < sends.len() && sends[sent] <= now_us {
                        asked = Some(name);
                        sent += 1;
                    }
                    taken.extend(body.step(now_us, asked).0);
                    now_us += BODY_STEP_US;
                }
                taken
            };
            // The clip's length after the first, as it was sent before: dropped.
            assert_eq!(taken(length), [true, false], "{name} at {phase_us}");
            // Sooner, while it plays: dropped too.
            assert_eq!(taken(length - BODY_STEP_US), [true, false]);
            // The program's: taken.
            assert_eq!(
                taken(length + CLIP_GAP_US),
                [true, true],
                "{name} at {phase_us}"
            );
        }
    }
}

/// How the model's line ends in [`the_dance`].
#[derive(Clone, Copy, PartialEq)]
enum Line {
    /// Spoken from 200 to 700 ms after the call, then `resting`.
    Said,
    /// Spoken for 30 ms, then thinking for 1.5 s while it plays, then
    /// `resting`.
    Thinking,
    /// Spoken from 200 to 700 ms, and cut: the person spoke over it, so
    /// the voice listens.
    Cut,
}

/// What the program did with one call of `nods` with `long`, against a
/// stand-in body that drops a gesture sent while one plays and says on
/// `body/gesture` what `says`: with `push`, a hand pushes the body 1 s into
/// the first nod. When the voice stopped, each gesture the body began and
/// when, the actions part's calls, the run's done event, and how many
/// gestures reached the body. No gesture is dropped for another playing.
fn the_dance(
    push: bool,
    ending: Line,
) -> (Instant, Vec<(Instant, String)>, Vec<String>, SkillEvent) {
    let (stopped, played, lights, done, _) = the_dance_saying(push, ending, Says::BeganOver);
    (stopped, played, lights, done)
}

fn the_dance_saying(
    push: bool,
    ending: Line,
    says: Says,
) -> (
    Instant,
    Vec<(Instant, String)>,
    Vec<String>,
    SkillEvent,
    usize,
) {
    the_dance_asking(push, ending, says, false)
}

/// The same, with `LAMP_BODY_REQUESTS=on` when `body_requests` (#940).
fn the_dance_asking(
    push: bool,
    ending: Line,
    says: Says,
    body_requests: bool,
) -> (
    Instant,
    Vec<(Instant, String)>,
    Vec<String>,
    SkillEvent,
    usize,
) {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("dn")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    fs::create_dir(root.join("skills")).unwrap();
    skill_with(&root.join("skills"), "nods", NODS);
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let actions = UnixListener::bind(root.join("actions.sock")).unwrap();
    let (called, lights) = mpsc::channel();
    let ring = bus.clone();
    std::thread::spawn(move || {
        let mut shown = ring
            .publisher_declared("light/state", &Light::declaration(), "ring")
            .unwrap();
        let mut light = Light::new(true, "blue", 3).unwrap();
        for stream in actions.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let call: serde_json::Value = serde_json::from_str(&line).unwrap();
            if call["name"] == "light_colour" {
                light.colour = call["args"]["colour"].as_str().unwrap().into();
            }
            shown.publish_json(1, &light).unwrap();
            (&stream)
                .write_all(b"{\"ok\":true,\"result\":{}}\n")
                .unwrap();
            let _ = called.send(line);
        }
    });
    let mood = UnixDatagram::bind(root.join("mood.sock")).unwrap();
    let (moved, gestures) = mpsc::channel();
    let (lost, dropped) = mpsc::channel();
    let clock = Clock::at(root);
    let (took, taken) = mpsc::channel();
    StandInBody::run(mood, &bus, says, clock.clone(), took, moved, lost);
    let tools = root.join("tools");
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    command
        .args([
            "--dir".as_ref(),
            root.join("skills").as_os_str(),
            "--bus-root".as_ref(),
            root.join("bus").as_os_str(),
            "--actions-socket".as_ref(),
            root.join("actions.sock").as_os_str(),
            "--mood-socket".as_ref(),
            root.join("mood.sock").as_os_str(),
            "--tools-dir".as_ref(),
            tools.as_os_str(),
        ])
        .args(clock.args())
        .env_remove("LAMP_BODY_REQUESTS");
    if body_requests {
        command.env("LAMP_BODY_REQUESTS", "on");
    }
    let _running = Running(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(20);
    while !tools.join("tools.json").exists() {
        assert!(Instant::now() < deadline, "no tools file");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut voice = bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    let mut hand = bus
        .publisher_declared(Hand::TOPIC, &Hand::declaration(), "body")
        .unwrap();
    hand.publish_json(1, &serde_json::json!({"pushed": false, "since_us": 0}))
        .unwrap();
    voice
        .publish_json(1, &serde_json::json!({"state": "thinking"}))
        .unwrap();
    bus.publisher_declared("light/state", &Light::declaration(), "ring")
        .unwrap()
        .publish_json(1, &Light::new(true, "blue", 3).unwrap())
        .unwrap();
    let stream = std::os::unix::net::UnixStream::connect(tools.join("call.sock")).unwrap();
    writeln!(
        &stream,
        r#"{{"op":"call","name":"nods","args":{{"long":true}}}}"#
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).unwrap();
    assert!(line.contains("Say you will."), "{line}");
    std::thread::sleep(Duration::from_millis(200));
    voice
        .publish_json(1, &serde_json::json!({"state": "speaking"}))
        .unwrap();
    if ending == Line::Thinking {
        // As lamp-06c1 did on 2026-10-16: speaking for a moment, then
        // thinking while the line still plays.
        std::thread::sleep(Duration::from_millis(30));
        voice
            .publish_json(1, &serde_json::json!({"state": "thinking"}))
            .unwrap();
        clock.skip(Duration::from_millis(1_500));
        // The program looks at the voice every 20 ms.
        std::thread::sleep(Duration::from_millis(100));
    } else {
        std::thread::sleep(Duration::from_millis(500));
    }
    // Nothing moves or lights while the lamp speaks its line.
    assert!(gestures.try_recv().is_err());
    assert!(lights.try_recv().is_err());
    // A line said to its end leaves the voice resting (lamp-06c1's asks 8,
    // 14 and 16); one spoken over, listening.
    let after = if ending == Line::Cut {
        "listening"
    } else {
        "resting"
    };
    voice
        .publish_json(1, &serde_json::json!({ "state": after }))
        .unwrap();
    let stopped = clock.now();
    let nod = Duration::from_micros(clip_us("nod").unwrap());
    let gap = Duration::from_micros(CLIP_GAP_US);
    let mut played = Vec::new();
    let mut reached = 0;
    if ending != Line::Cut {
        // Sent at the line's end, and taken (or said dropped) at once.
        taken.recv_timeout(Duration::from_secs(5)).unwrap();
        reached += 1;
        if !matches!(says, Says::Dropped | Says::Busy) {
            played.push(gestures.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        if push {
            clock.skip_to(stopped + Duration::from_secs(1));
            hand.publish_json(1, &serde_json::json!({"pushed": true, "since_us": 5}))
                .unwrap();
        } else if says == Says::NoOver {
            // The clip ends and nothing says so: past its length, the gap
            // and the 3 s the program waits for `over`.
            clock.skip_to(played[0].0 + nod + gap + Duration::from_millis(3_100));
        } else if matches!(says, Says::BeganOver | Says::AnothersFirst | Says::LastBusy) {
            // Nothing more is sent while the clip plays.
            std::thread::sleep(Duration::from_millis(300));
            assert!(taken.try_recv().is_err());
            // It ends at its length: the body says `over`, and the program
            // sends the next, which the body takes.
            clock.skip_to(played[0].0 + nod);
            let second = taken.recv_timeout(Duration::from_secs(5)).unwrap();
            reached += 1;
            if says == Says::LastBusy {
                // Said dropped as it came: the run ends at its length.
                clock.skip_to(second + nod + gap);
            } else {
                played.push(gestures.recv_timeout(Duration::from_secs(5)).unwrap());
                // The run is done once that one has played too.
                clock.skip_to(played[1].0 + nod + gap);
            }
        }
    }
    let done = loop {
        assert!(Instant::now() < deadline, "the run never ended");
        if let Some(message) = events.recv(Duration::from_millis(100)).unwrap() {
            let event: SkillEvent = message.parse().unwrap();
            if event.stage == SkillStage::Done {
                break event;
            }
        }
    };
    // The body steps on a little after the run: a gesture it dropped is
    // seen.
    std::thread::sleep(Duration::from_millis(3 * BODY_STEP_US / 1_000));
    let dropped: Vec<_> = dropped.try_iter().collect();
    assert!(dropped.is_empty(), "dropped {dropped:?}; {}", done.why);
    played.extend(gestures.try_iter());
    reached += taken.try_iter().count();
    (stopped, played, lights.try_iter().collect(), done, reached)
}

#[test]
fn a_dance_waits_for_the_line_plays_twice_and_gives_the_light_back_at_its_end() {
    let (stopped, gestures, lights, done) = the_dance(false, Line::Said);
    let nod = Duration::from_micros(clip_us("nod").unwrap());
    // Both taken by a body that drops a gesture sent while one plays.
    assert_eq!(gestures.len(), 2, "{gestures:?}; {}", done.why);
    assert!(
        gestures
            .iter()
            .all(|(_, text)| text == r#"{"gesture":"nod"}"#)
    );
    // The first within 300 ms of the line's end (#999's target); the second
    // once the body said the first was over, at its length.
    let first = gestures[0].0 - stopped;
    assert!(first < Duration::from_millis(300), "{first:?}");
    let between = gestures[1].0 - gestures[0].0;
    assert!(
        between >= nod && between < nod + Duration::from_millis(300),
        "{between:?}"
    );
    assert!(lights[0].contains("warm"), "{lights:?}");
    assert!(lights.last().unwrap().contains("blue"), "{lights:?}");
    assert!(
        done.why.contains("light given back to on blue at 3"),
        "{}",
        done.why
    );
}

#[test]
fn a_clip_the_body_said_dropped_is_followed_by_nothing() {
    // `body/gesture` said `dropped` for the first: nothing more is sent.
    let (_, played, _, done, reached) = the_dance_saying(false, Line::Said, Says::Dropped);
    assert!(played.is_empty(), "{played:?}");
    assert_eq!(reached, 1, "{}", done.why);
    assert!(
        done.why.contains(
            "gesture nod: not sent, the body dropped nod (body/gesture), so nothing more is sent"
        ),
        "{}",
        done.why
    );
}

/// #940: under arbitration the body names whose a drop was, why and who
/// holds the body; the run says so, and takes no other skill's drop of the
/// same clip as its own.
#[test]
fn a_clip_dropped_for_another_skill_says_why_and_who_holds_the_body() {
    let (_, played, _, done, reached) = the_dance_asking(false, Line::Said, Says::Busy, true);
    assert!(played.is_empty(), "{played:?}");
    assert_eq!(reached, 1, "{}", done.why);
    assert!(
        done.why.contains(
            "gesture nod: not sent, the body dropped nod (dropped: busy, skill, held by \
             skill:stretch-break), so nothing more is sent"
        ),
        "{}",
        done.why
    );
    // The run's last clip has no next to wait on it: said at the run's end.
    let (_, played, _, done, reached) = the_dance_asking(false, Line::Said, Says::LastBusy, true);
    assert_eq!((played.len(), reached), (1, 2), "{}", done.why);
    assert!(
        done.why
            .contains("the body dropped nod (dropped: busy, skill, held by skill:stretch-break)"),
        "{}",
        done.why
    );
    // Without the skill's switch (and so on a product lamp, the body's off
    // too), the run says nothing new of a last clip dropped (the reviewer
    // on #1473).
    let (_, played, _, done, _) = the_dance_asking(false, Line::Said, Says::LastBusy, false);
    assert_eq!(played.len(), 1, "{}", done.why);
    assert!(!done.why.contains("the body dropped"), "{}", done.why);
    // Another skill's nod dropped is not this run's: both are played.
    let (_, played, _, done, reached) =
        the_dance_asking(false, Line::Said, Says::AnothersFirst, true);
    assert_eq!((played.len(), reached), (2, 2), "{}", done.why);
    assert!(!done.why.contains("dropped"), "{}", done.why);
}

#[test]
fn a_clip_ended_for_a_higher_ask_is_followed_by_nothing() {
    // The first began, then the body ended it for the conversation's
    // gesture and said `dropped` (`ended: by conversation`): no second.
    let (_, played, _, done, reached) = the_dance_saying(false, Line::Said, Says::EndedBy);
    assert_eq!(played.len(), 1, "{played:?}");
    assert_eq!(reached, 1, "{}", done.why);
    assert!(
        done.why.contains(
            "gesture nod: not sent, the body dropped nod (body/gesture), so nothing more is sent"
        ),
        "{}",
        done.why
    );
}

#[test]
fn with_no_over_from_the_body_the_second_clip_is_not_sent() {
    // The first played, and `body/gesture` never said it was over: past its
    // length and 3 s more, the second is not sent, and `done` says why.
    let (_, played, _, done, reached) = the_dance_saying(false, Line::Said, Says::NoOver);
    assert_eq!(played.len(), 1, "{played:?}");
    assert_eq!(reached, 1, "{}", done.why);
    assert!(
        done.why.contains(
            "gesture nod: not sent, the body said no over for nod on body/gesture within"
        ),
        "{}",
        done.why
    );
}

/// #940's skills half: with `LAMP_BODY_REQUESTS=on`, each gesture a skill
/// sends says who asks, `skill:<name>` at `skill`, as a body with
/// `--arbitration on` weighs it. Without it, as before (the test above).
#[test]
fn with_body_requests_each_gesture_a_skill_sends_says_who_asks() {
    let (_, gestures, _, done, _) = the_dance_asking(false, Line::Said, Says::BeganOver, true);
    assert_eq!(gestures.len(), 2, "{gestures:?}; {}", done.why);
    for (_, text) in &gestures {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(text).unwrap(),
            serde_json::json!({"gesture": "nod", "source": "skill:nods", "priority": "skill"}),
        );
    }
}

#[test]
fn a_hand_on_the_body_ends_the_dance_and_the_clip_is_not_sent_again() {
    // The body lets the clip go under the hand and says it `over`: the
    // second is not sent all the same.
    let (_, gestures, lights, done, reached) = the_dance_saying(true, Line::Said, Says::BeganOver);
    assert_eq!(gestures.len(), 1, "{gestures:?}; {}", done.why);
    assert_eq!(reached, 1, "{}", done.why);
    assert!(
        done.why
            .contains("gesture nod: not sent, the body was pushed"),
        "{}",
        done.why
    );
    assert!(lights.last().unwrap().contains("blue"), "{lights:?}");
}

#[test]
fn thinking_after_a_moment_of_speaking_is_still_the_line() {
    // Nothing moves in the 1.5 s of thinking (checked in `the_dance`); the
    // clip comes when the turn ends.
    let (stopped, gestures, _, done) = the_dance(false, Line::Thinking);
    let first = gestures[0].0 - stopped;
    assert!(
        first < Duration::from_millis(300),
        "{first:?}; {}",
        done.why
    );
}

#[test]
fn a_line_the_person_spoke_over_is_not_followed_by_the_dance() {
    // The voice went from speaking to listening: the lamp stops and
    // listens. No clip, no glow, and so no light to give back.
    let (_, gestures, lights, done) = the_dance(false, Line::Cut);
    assert!(gestures.is_empty(), "{gestures:?}; {}", done.why);
    assert!(lights.is_empty(), "{lights:?}; {}", done.why);
    assert!(
        done.why.contains(
            "the line was cut (the voice went from speaking to listening): nothing after it is made"
        ),
        "{}",
        done.why
    );
}

/// Dance's `[again]` (#999), on a tool with a short clip.
const AGAIN: &str = r#"
tool = "jig"
[again]
within = "45s"
if_heard = ["Dance", "again", "one more"]
told = "You just danced."
[[do]]
body = { gesture = "acknowledge" }
"#;

/// The same tool without `[again]`.
const NO_AGAIN: &str = r#"
tool = "jig"
[[do]]
body = { gesture = "acknowledge" }
"#;

#[test]
fn the_loader_refuses_an_again_it_cannot_use() {
    let dir = tempfile::tempdir().unwrap();
    for (from, to, why) in [
        (
            "tool = \"jig\"",
            "[trigger]\ntopic = \"humans/faces/tracked\"\n[ask]\npolicy = \"x\"",
            "a skill with one",
        ),
        ("within = \"45s\"", "within = \"soon\"", "not a duration"),
        (
            "if_heard = [\"Dance\", \"again\", \"one more\"]",
            "if_heard = []",
            "at least one word",
        ),
        ("\"one more\"", "\" \"", "none of them empty"),
        ("told = \"You just danced.\"", "told = \"\"", "told is"),
        ("told = ", "said = ", "unknown field"),
    ] {
        assert!(AGAIN.contains(from), "{from}");
        let error = load(&skill_with(
            &dir.path().join(why.replace([' ', '"', '\''], "_")),
            "jig",
            &AGAIN.replace(from, to),
        ))
        .unwrap_err();
        assert!(error.contains(why), "{to}: {error}");
    }
    // Without told, a general one.
    let skill = load(&skill_with(
        &dir.path().join("plain"),
        "jig",
        &AGAIN.replace("told = \"You just danced.\"\n", ""),
    ))
    .unwrap();
    let mut engine = Engine::new(vec![skill], false);
    assert!(engine.tool("jig", &serde_json::Value::Null, 0).is_ok());
    let why = engine.tool("jig", &serde_json::Value::Null, 1).unwrap_err();
    assert!(why.contains("you just did this"), "{why}");
    // Dance's own: 45 s.
    let dance = load(&skills_root().join("dance")).unwrap();
    let mut engine = Engine::new(vec![dance], false);
    assert_eq!(engine.topics(), ["voice/heard"]);
    let mut dance = |at_us: u64| engine.tool("dance", &serde_json::Value::Null, at_us);
    assert!(dance(0).is_ok());
    let why = dance(28_000_000).unwrap_err();
    assert!(why.starts_with("You just danced."), "{why}");
    assert!(dance(44_999_999).is_err());
    assert!(dance(45_000_000).is_ok());
    // Its entries are whole words: "guidance" asks for nothing, "dancing"
    // does (Seventh, #1012).
    let heard = |text: &str| serde_json::json!({"text": text, "finished": true}).to_string();
    let null = serde_json::Value::Null;
    engine.observe(
        "voice/heard",
        46_000_000,
        heard("Can you give me some guidance?").as_bytes(),
        None,
    );
    assert!(engine.tool("dance", &null, 47_000_000).is_err());
    engine.observe(
        "voice/heard",
        48_000_000,
        heard("I'm against that.").as_bytes(),
        None,
    );
    assert!(engine.tool("dance", &null, 49_000_000).is_err());
    engine.observe(
        "voice/heard",
        50_000_000,
        heard("Keep dancing!").as_bytes(),
        None,
    );
    assert!(engine.tool("dance", &null, 51_000_000).is_ok());
    // What counts began after the last run began (the reviewer, 01:56):
    // the ask's own words, finished after its call, unlock nothing.
    let piece = |text: &str, finished: bool| {
        serde_json::json!({"text": text, "finished": finished}).to_string()
    };
    const S: u64 = 1_000_000;
    let hear = |engine: &mut Engine, at_us: u64, text: &str, finished: bool| {
        engine.observe("voice/heard", at_us, piece(text, finished).as_bytes(), None);
    };
    // After the while: "Lamp, dance", the call, " for me." 300 ms on, and a
    // call 15 s on with nothing new heard: refused.
    hear(&mut engine, 100 * S, "Lamp, dance", false);
    assert!(engine.tool("dance", &null, 100 * S + 100_000).is_ok());
    hear(&mut engine, 100 * S + 400_000, " for me.", true);
    assert!(engine.tool("dance", &null, 115 * S + 100_000).is_err());
    // "Lamp, show me a dance", never finished, the call, then "¿Qué?" 15 s
    // later, joined to it: refused.
    hear(&mut engine, 200 * S, "Lamp, show me a", false);
    hear(&mut engine, 200 * S + 300_000, " dance", false);
    assert!(engine.tool("dance", &null, 200 * S + 500_000).is_ok());
    hear(&mut engine, 215 * S, " ¿Qué?", true);
    assert!(engine.tool("dance", &null, 216 * S).is_err());
    // A fresh "dance again" after the run: it runs.
    hear(&mut engine, 220 * S, "Dance again", false);
    hear(&mut engine, 220 * S + 300_000, "!", true);
    assert!(engine.tool("dance", &null, 221 * S).is_ok());
}

#[test]
fn within_its_while_a_tool_runs_again_only_when_what_was_heard_asks_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let skill = load(&skill_with(dir.path(), "jig", AGAIN)).unwrap();
    let mut engine = Engine::new(vec![skill], false);
    assert_eq!(engine.topics(), ["voice/heard"]);
    const S: u64 = 1_000_000;
    let call = |engine: &mut Engine, at_us: u64| {
        engine
            .tool("jig", &serde_json::Value::Null, at_us)
            .map(|run| run.calls)
    };
    let hear = |engine: &mut Engine, at_us: u64, text: &str, finished: bool| {
        let speech = serde_json::json!({"text": text, "finished": finished});
        assert!(
            engine
                .observe("voice/heard", at_us, speech.to_string().as_bytes(), None)
                .is_empty()
        );
    };
    // Heard before the first run: it counts for none after it.
    hear(&mut engine, S, "dance", true);
    assert_eq!(
        call(&mut engine, 2 * S).unwrap(),
        [Call::Gesture("acknowledge".into())]
    );
    // Nothing heard since: refused, with the model's words.
    assert_eq!(call(&mut engine, 3 * S).unwrap_err(), "You just danced.");
    // A noise heard as a question: refused.
    hear(&mut engine, 20 * S, "¿Qué?", true);
    assert_eq!(call(&mut engine, 30 * S).unwrap_err(), "You just danced.");
    // Words still coming count, in any case.
    hear(&mut engine, 31 * S, "DANCE AGA", false);
    assert!(call(&mut engine, 32 * S).is_ok());
    // That ran: the while begins again from it, and what was heard before
    // it no longer counts, nor does its end, heard after it: it began
    // before the run.
    assert!(call(&mut engine, 33 * S).is_err());
    hear(&mut engine, 34 * S, "IN!", true);
    assert!(call(&mut engine, 35 * S).is_err());
    hear(&mut engine, 40 * S, "One more!", true);
    assert!(call(&mut engine, 41 * S).is_ok());
    // The newest heard is what counts: "again", then a noise.
    hear(&mut engine, 42 * S, "again", true);
    hear(&mut engine, 43 * S, "hm", false);
    assert!(call(&mut engine, 44 * S).is_err());
    // After the while, anything: 45 s from when the last run began.
    assert!(call(&mut engine, 86 * S - 1).is_err());
    assert!(call(&mut engine, 86 * S).is_ok());
    // After the while, a run. Then a request heard in pieces, joined as
    // the voice joins them: "Lamp, do it again," and then " please.", the
    // last piece finishing it.
    assert!(call(&mut engine, 200 * S).is_ok());
    hear(&mut engine, 201 * S, "Lamp, do it again,", false);
    hear(&mut engine, 201 * S + 300_000, " please.", true);
    assert!(call(&mut engine, 202 * S).is_ok());
    // "one" and then " more": the phrase across two pieces.
    hear(&mut engine, 203 * S, "Can I have one", false);
    hear(&mut engine, 203 * S + 400_000, " more?", true);
    assert!(call(&mut engine, 204 * S).is_ok());
    // A piece after a finished one begins another: "again" said, then a
    // noise, is the noise.
    hear(&mut engine, 205 * S, "again", true);
    hear(&mut engine, 206 * S, "¿Qué?", true);
    assert!(call(&mut engine, 207 * S).is_err());
    // A piece left unfinished for more than 20 s is not joined to the next.
    hear(&mut engine, 208 * S, "Do it again", false);
    assert!(call(&mut engine, 210 * S).is_ok());
    hear(&mut engine, 211 * S, "one", false);
    hear(&mut engine, 232 * S, " more", true);
    assert!(call(&mut engine, 233 * S).is_err());
    // Whole words: "dance" is not in "guidance", nor "again" in "against".
    hear(&mut engine, 234 * S, "Can you give me some guidance?", true);
    assert!(call(&mut engine, 235 * S).is_err());
    hear(&mut engine, 236 * S, "I'm against that.", true);
    assert!(call(&mut engine, 237 * S).is_err());
    hear(&mut engine, 238 * S, "Dance, again!", true);
    assert!(call(&mut engine, 239 * S).is_ok());
    // A tool without [again] does not listen to what was heard.
    let plain = load(&skill_with(&dir.path().join("plain"), "jig", NO_AGAIN)).unwrap();
    let mut engine = Engine::new(vec![plain], false);
    assert!(engine.topics().is_empty());
    assert!(call(&mut engine, 0).is_ok());
    assert!(call(&mut engine, 1).is_ok());
}

/// `[again]` in the program, with a while short enough to wait out.
const AGAIN_SHORT: &str = r#"
tool = "jig"
still = "I'm still dancing."
[again]
within = "6s"
if_heard = ["dance", "again", "one more"]
told = "You just danced."
[[do]]
body = { gesture = "acknowledge" }
"#;

#[test]
fn the_program_dances_again_on_dance_again_but_not_on_a_noise() {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("ag")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    fs::create_dir(root.join("skills")).unwrap();
    skill_with(&root.join("skills"), "jig", AGAIN_SHORT);
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let mood = UnixDatagram::bind(root.join("mood.sock")).unwrap();
    let (moved, gestures) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 512];
        while let Ok(length) = mood.recv(&mut buffer) {
            let _ = moved.send(String::from_utf8_lossy(&buffer[..length]).into_owned());
        }
    });
    let tools = root.join("tools");
    let clock = Clock::at(root);
    let _running = Running(
        Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args([
                "--dir".as_ref(),
                root.join("skills").as_os_str(),
                "--bus-root".as_ref(),
                root.join("bus").as_os_str(),
                "--mood-socket".as_ref(),
                root.join("mood.sock").as_os_str(),
                "--tools-dir".as_ref(),
                tools.as_os_str(),
            ])
            .args(clock.args())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !tools.join("tools.json").exists()
        || bus.subscribers(Speech::TOPICS[0]).unwrap().is_empty()
    {
        assert!(Instant::now() < deadline, "not loaded");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut heard = bus
        .publisher_declared(Speech::TOPICS[0], &Speech::declaration(), "voice")
        .unwrap();
    let mut hear = |text: &str, finished: bool| {
        heard
            .publish_json(1, &serde_json::json!({"text": text, "finished": finished}))
            .unwrap();
    };
    let call = || {
        let stream = std::os::unix::net::UnixStream::connect(tools.join("call.sock")).unwrap();
        writeln!(&stream, r#"{{"op":"call","name":"jig","args":{{}}}}"#).unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    };
    let gesture = || gestures.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut done = || {
        loop {
            assert!(Instant::now() < deadline, "the run never ended");
            if let Some(message) = events.recv(Duration::from_millis(100)).unwrap()
                && message.parse::<SkillEvent>().unwrap().stage == SkillStage::Done
            {
                break;
            }
        }
    };
    // A run is done once its clip has played, from when it was sent.
    let clip = Duration::from_micros(clip_us("acknowledge").unwrap());
    let played = clip + Duration::from_micros(CLIP_GAP_US);
    // The dance.
    let answer = call();
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(gesture(), r#"{"gesture":"acknowledge"}"#);
    clock.skip(played);
    done();
    // A noise heard as "¿Qué?", and the model calls again: refused with the
    // model's words, and nothing moves.
    hear("¿Qué?", true);
    let answer = call();
    assert_eq!(answer["ok"], false, "{answer}");
    assert_eq!(answer["error"], "You just danced.");
    assert!(gestures.recv_timeout(Duration::from_millis(300)).is_err());
    // "Dance again!", still coming: it dances.
    hear("Dance again!", false);
    let again = clock.now();
    let answer = call();
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(gesture(), r#"{"gesture":"acknowledge"}"#);
    let sent = clock.now();
    // "One more!" 1 s into it, while it plays: refused with the model's
    // words, nothing more moves, and it is no run.
    assert!(clip > Duration::from_millis(1_500), "{clip:?}");
    clock.skip_to(again + Duration::from_secs(1));
    hear("One more!", true);
    let answer = call();
    assert_eq!(answer["ok"], false, "{answer}");
    assert_eq!(answer["error"], "I'm still dancing.");
    clock.skip_to(sent + played);
    done();
    assert!(gestures.recv_timeout(Duration::from_millis(300)).is_err());
    // A noise after it: the while is still the dance's, from `again`, not
    // from the call refused 1 s later.
    hear("¿Qué?", true);
    // After the while, with only the noise heard since: it dances.
    clock.skip_to(again + Duration::from_millis(6_300));
    let answer = call();
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(gesture(), r#"{"gesture":"acknowledge"}"#);
}

/// A dim held while someone is in view, given back when they go (#1021).
const DIM_WHILE: &str = r#"
[trigger]
topic = "humans/faces/tracked"
rearm = "leaves"
[ask]
policy = "nudge"
[[do]]
light = { dim = 1, until = "ends" }
"#;

#[test]
fn a_dim_until_it_ends_is_held_and_given_back_when_the_trigger_ends() {
    let seen = the_program_then(DIM_WHILE, true, &[], 3, 1, 4, Some(None));
    let done = &seen.events[3];
    assert_eq!(done.stage, SkillStage::Done, "{:?}", seen.events);
    assert!(
        done.why.contains("held until its trigger ended")
            && done.why.contains("light given back to on blue at 3"),
        "{}",
        done.why
    );
    assert_eq!(seen.lights.len(), 2, "{:?}", seen.lights);
    assert!(seen.lights[0].contains("light_dimmer"), "{:?}", seen.lights);
    assert!(
        seen.lights[1].contains("light_brighter"),
        "{:?}",
        seen.lights
    );
}

/// With the ring asked (#1132), a dim held until its trigger ends is asked
/// of the ring, kept while it holds, and given back the same way.
#[test]
fn a_dim_until_it_ends_asked_of_the_ring_is_given_back_when_the_trigger_ends() {
    let seen = the_program_timed(
        DIM_WHILE,
        true,
        &[],
        3,
        1,
        4,
        (Ring::Asked, After::Leaves(None)),
        false,
    );
    assert!(seen.lights.is_empty(), "{:?}", seen.lights);
    let done = &seen.events[3].why;
    assert!(done.contains("held until its trigger ended"), "{done}");
    assert!(seen.requests.len() >= 2, "{:?}", seen.requests);
    let (asked, back) = (&seen.requests[0], seen.requests.last().unwrap());
    assert_eq!(asked.light, Light::new(true, "blue", 2).unwrap());
    assert!(
        !back.claim.holds_at("boot-test", lamp_ipc::monotonic_us()),
        "{back:?}"
    );
}

/// The light going out while someone is in view (#1677's step, on a trigger
/// a test can hold), given back when they go.
const OUT_WHILE: &str = r#"
[trigger]
topic = "humans/faces/tracked"
rearm = "leaves"
[ask]
policy = "light"
[[do]]
light = { out = true, over = "30s", until = "ends" }
"#;

/// #1677: the light going out is asked of the ring as the light shown, off,
/// with its fade, renewed with it while it holds, and given back when the
/// trigger ends; the person's light is never touched.
#[test]
fn the_light_going_out_is_asked_of_the_ring_with_its_fade_and_given_back_when_the_trigger_ends() {
    let seen = the_program_timed(
        OUT_WHILE,
        true,
        &[],
        3,
        1,
        4,
        (Ring::Asked, After::Leaves(None)),
        false,
    );
    assert!(seen.lights.is_empty(), "{:?}", seen.lights);
    let done = &seen.events[3].why;
    assert!(
        done.contains("light out: asked of the ring, faded into over 30000 ms")
            && done.contains("held until its trigger ended")
            && done.contains("light given back"),
        "{done}"
    );
    assert!(seen.requests.len() >= 2, "{:?}", seen.requests);
    let (asked, back) = (&seen.requests[0], seen.requests.last().unwrap());
    assert_eq!(asked.light, Light::new(false, "blue", 3).unwrap());
    assert_eq!((asked.fade_ms, asked.pattern), (Some(30_000), None));
    assert!(asked.check().is_ok(), "{asked:?}");
    assert_eq!(back.light, asked.light);
    assert!(
        !back.claim.holds_at("boot-test", lamp_ipc::monotonic_us()),
        "{back:?}"
    );
}

/// #1677: without the ring's switch the light is left alone, never turned
/// off as the person's own, and the run says why.
#[test]
fn without_the_ring_the_light_is_never_put_out() {
    let seen = the_program_then(OUT_WHILE, true, &[], 3, 1, 4, Some(None));
    assert!(seen.lights.is_empty(), "{:?}", seen.lights);
    let done = seen
        .events
        .iter()
        .find(|event| event.stage == SkillStage::Done)
        .expect("done");
    assert!(
        done.why.contains(
            "light out: left alone, since only the ring puts it out (LAMP_LIGHT_REQUESTS=on)"
        ),
        "{}",
        done.why
    );
    assert!(!done.why.contains("held until"), "{}", done.why);
}

/// #1677: `out` is its own light step: only with `over` (1 to 60 s) and
/// `until`; `over` only with it; it is named as a test names it.
#[test]
fn the_light_going_out_is_loaded_with_its_fade_and_refused_with_anything_else() {
    let dir = tempfile::tempdir().unwrap();
    let with = |name: &str, light: &str| {
        skill_with(
            dir.path(),
            name,
            &OUT_WHILE.replace(
                r#"light = { out = true, over = "30s", until = "ends" }"#,
                &format!("light = {light}"),
            ),
        )
    };
    let skill = load(&with(
        "out",
        r#"{ out = true, over = "30s", until = "ends" }"#,
    ))
    .unwrap();
    let out = Call::Out {
        over_ms: Some(30_000),
        until_ends: true,
    };
    assert!(out.held_until_ends());
    assert_eq!(
        out.named(),
        (
            "light",
            serde_json::json!({"out": true, "over_ms": 30000, "until": "ends"})
        )
    );
    let quick = Call::Out {
        over_ms: None,
        until_ends: false,
    };
    assert!(!quick.held_until_ends());
    assert_eq!(quick.named().1, serde_json::json!({"out": true}));
    assert_eq!(skill.triggers.len(), 1);
    load(&with("quick", "{ out = true }")).unwrap();
    load(&with("second", r#"{ out = true, over = "1s" }"#)).unwrap();
    load(&with("minute", r#"{ out = true, over = "60s" }"#)).unwrap();
    for (name, light, why) in [
        (
            "short",
            r#"{ out = true, over = "999ms" }"#,
            "from 1 s to 60 s",
        ),
        (
            "long",
            r#"{ out = true, over = "61s" }"#,
            "from 1 s to 60 s",
        ),
        (
            "coloured",
            r#"{ out = true, colour = "warm" }"#,
            "out takes no colour",
        ),
        ("dimmed", "{ out = true, dim = 1 }", "out takes no colour"),
        (
            "levelled",
            "{ out = true, level = 2 }",
            "out takes no colour",
        ),
        (
            "lasting",
            r#"{ out = true, for = "4s" }"#,
            "out takes no colour",
        ),
        ("false", "{ out = false }", "out is true or left out"),
        (
            "slow-dim",
            r#"{ dim = 1, over = "30s" }"#,
            "over goes with out = true",
        ),
        (
            "slow-colour",
            r#"{ colour = "warm", over = "30s" }"#,
            "over goes with out = true",
        ),
    ] {
        let refused = load(&with(name, light)).unwrap_err();
        assert!(refused.contains(why), "{name}: {refused}");
    }
}

#[test]
fn a_light_someone_changed_while_it_was_held_is_left_as_it_is() {
    let theirs = Light::new(true, "red", 5).unwrap();
    let seen = the_program_then(DIM_WHILE, true, &[], 3, 1, 4, Some(Some(theirs)));
    let done = &seen.events[3];
    assert!(
        done.why
            .contains("changed meanwhile or not known, so left as it is"),
        "{}",
        done.why
    );
    assert_eq!(seen.lights.len(), 1, "{:?}", seen.lights);
}

/// A dim held while someone is in view, its skill's folder removed while
/// they are still there: the reload gives the light back and the run is
/// done (#1066's P3).
#[test]
fn a_dim_held_until_it_ends_is_given_back_when_its_skill_is_removed() {
    let seen = the_program_timed(
        DIM_WHILE,
        true,
        &[],
        3,
        1,
        4,
        (Ring::Off, After::Removed),
        false,
    );
    let done = &seen.events[3];
    assert_eq!(done.stage, SkillStage::Done, "{:?}", seen.events);
    assert!(
        done.why.contains("held until its trigger ended")
            && done.why.contains("light given back to on blue at 3"),
        "{}",
        done.why
    );
    assert_eq!(seen.lights.len(), 2, "{:?}", seen.lights);
    assert!(seen.lights[0].contains("light_dimmer"), "{:?}", seen.lights);
    assert!(
        seen.lights[1].contains("light_brighter"),
        "{:?}",
        seen.lights
    );
}

/// Stuffy air's trigger and quiet hours, as #1021 asks, with no condition.
const STUFFY: &str = r#"
[trigger]
topic = "air/state"
field = "co2_ppm"
above = 1200
for = "10m"
gap = "1m"
rearm = { below = 1000, for = "10m" }
[quiet]
from = "22:00"
to = "08:00"
[ask]
policy = "nudge"
[[do]]
body = { gesture = "sleepy" }
light = { dim = 1, until = "ends" }
"#;

/// CO2 each minute from `from` up to `to` minutes, as (minute, ppm).
fn co2(from: u64, to: u64, ppm: u32) -> Vec<(u64, u32)> {
    (from..to).map(|minute| (minute, ppm)).collect()
}

/// The minute of each run of `skill` and its calls, the engine told it is
/// `clock` (minutes since local midnight) at the first message, if given.
fn stuffy_runs(skill: &Path, air: &[(u64, u32)], clock: Option<u64>) -> Vec<(u64, Vec<Call>)> {
    let mut engine = Engine::new(vec![load(skill).unwrap()], true);
    if let Some(clock) = clock {
        engine.clock(0, 0, clock * 60_000);
    }
    let mut runs = Vec::new();
    for (minute, ppm) in air {
        let payload = format!(r#"{{"co2_ppm":{ppm}}}"#);
        for run in engine.observe("air/state", minute * 60_000_000, payload.as_bytes(), None) {
            runs.push((*minute, run.calls));
        }
    }
    runs
}

#[test]
fn a_rearm_line_waits_for_the_field_to_stay_past_it_for_a_while() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "stuffy", STUFFY);
    let droop = vec![
        Call::Gesture("sleepy".into()),
        Call::Dim {
            steps: 1,
            until_ends: true,
        },
    ];
    // A spell, 9 minutes under 1,000, a spell: once, as the line was not
    // held for 10 minutes. The end of the spell that ran gives back what it
    // held; the spell that did not has nothing to give back.
    let mut air = co2(0, 30, 1300);
    air.extend(co2(30, 39, 900));
    air.extend(co2(39, 70, 1300));
    air.extend(co2(70, 80, 900));
    let runs = stuffy_runs(&skill, &air, Some(600));
    assert_eq!(runs, [(10, droop.clone()), (30, vec![Call::Ends])]);
    // Ten minutes under 1,000: the next spell has its own.
    let mut air = co2(0, 30, 1300);
    air.extend(co2(30, 41, 900));
    air.extend(co2(41, 70, 1300));
    air.extend(co2(70, 72, 900));
    let runs = stuffy_runs(&skill, &air, Some(600));
    let minutes: Vec<u64> = runs.iter().map(|run| run.0).collect();
    assert_eq!(minutes, [10, 30, 51, 70], "{runs:?}");
    assert_eq!(runs[2].1, droop);
}

#[test]
fn quiet_hours_keep_a_skill_still_and_so_does_a_time_of_day_not_known() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "stuffy", STUFFY);
    let air = co2(0, 30, 1300);
    // From 21:55, the tenth minute is 22:05: nothing, and nothing to end.
    let mut ending = air.clone();
    ending.extend(co2(30, 35, 800));
    assert!(stuffy_runs(&skill, &ending, Some(21 * 60 + 55)).is_empty());
    // From 07:55, 08:05 is past the night.
    assert_eq!(stuffy_runs(&skill, &air, Some(7 * 60 + 55)).len(), 1);
    // Past midnight, from 01:00: nothing.
    assert!(stuffy_runs(&skill, &air, Some(60)).is_empty());
    // Stuffy from 23:00 to 10:00: the chance that came at 23:10 is kept
    // through the night, and the lamp droops once, at 08:00.
    let mut night = co2(0, 660, 1300);
    night.extend(co2(660, 670, 800));
    let runs = stuffy_runs(&skill, &night, Some(23 * 60));
    let minutes: Vec<u64> = runs.iter().map(|run| run.0).collect();
    assert_eq!(minutes, [9 * 60, 660], "{runs:?}");
    assert_eq!(runs[1].1, [Call::Ends]);
    // An `appears` chance in the quiet hours is spent, not kept until 08:00.
    let appears = skill_with(
        dir.path(),
        "appears",
        "[trigger]\ntopic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\n\
         when = \"appears\"\n[quiet]\nfrom = \"22:00\"\nto = \"08:00\"\n\
         [ask]\npolicy = \"nudge\"\n[[do]]\nbody = { gesture = \"sleepy\" }\n",
    );
    assert!(stuffy_runs(&appears, &co2(0, 660, 1300), Some(23 * 60)).is_empty());
    // The time of day not known: never.
    assert!(stuffy_runs(&skill, &air, None).is_empty());
    // A skill without quiet hours needs no clock.
    let always = skill_with(
        dir.path(),
        "always",
        &STUFFY.replace("[quiet]\nfrom = \"22:00\"\nto = \"08:00\"\n", ""),
    );
    assert_eq!(stuffy_runs(&always, &air, None).len(), 1);
}

#[test]
fn the_loader_refuses_a_rearm_line_quiet_hours_or_an_end_it_cannot_use() {
    let dir = tempfile::tempdir().unwrap();
    let triggered = STUFFY.split("[[do]]").next().unwrap();
    for (from, to, why) in [
        (
            "rearm = { below = 1000, for = \"10m\" }",
            "rearm = { below = 1000, above = 1300 }",
            "one line",
        ),
        (
            "rearm = { below = 1000, for = \"10m\" }",
            "rearm = \"never\"",
            "\"leaves\", \"ends\" or",
        ),
        ("from = \"22:00\"", "from = \"10pm\"", "not a time of day"),
        ("to = \"08:00\"", "to = \"24:00\"", "not a time of day"),
        ("to = \"08:00\"", "to = \"22:00\"", "different times"),
        (triggered, "tool = \"droop\"\n", "the end of a trigger"),
        // A trigger and a tool: the tool's run would never be told its end.
        (
            triggered,
            &format!("tool = \"droop\"\n{triggered}"),
            "in a skill with one and no tool",
        ),
    ] {
        assert!(STUFFY.contains(from), "{from}");
        let skill = skill_with(
            &dir.path().join(why.replace([' ', '"', ','], "_")),
            "stuffy",
            &STUFFY.replace(from, to),
        );
        let error = load(&skill).unwrap_err();
        assert!(error.contains(why), "{to}: {error}");
    }
    // A rearm line needs a trigger with a field.
    let skill = skill_with(
        &dir.path().join("faces"),
        "faces",
        &DIM_WHILE.replace("rearm = \"leaves\"", "rearm = { below = 1 }"),
    );
    assert!(load(&skill).unwrap_err().contains("one line"));
}

const MINUTE: u64 = 60_000_000;

#[test]
fn a_held_run_ends_when_its_trigger_value_expires_or_its_topic_goes_silent() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "stuffy", STUFFY);
    let high = br#"{"co2_ppm":1300}"#;
    for (expires_ms, ends_after) in [(Some(120_000), 2), (None, 15)] {
        let mut engine = Engine::new(vec![load(&skill).unwrap()], true);
        engine.clock(0, 0, 600 * 60_000);
        let mut ran = 0;
        for minute in 0..30 {
            ran += engine
                .observe("air/state", minute * MINUTE, high, expires_ms)
                .len();
        }
        assert_eq!(ran, 1);
        // The last reading at minute 29: still held a minute before its end.
        assert!(engine.tick((29 + ends_after - 1) * MINUTE).is_empty());
        let ended = engine.tick((29 + ends_after) * MINUTE);
        assert_eq!(ended.len(), 1, "{expires_ms:?}");
        assert_eq!(ended[0].calls, [Call::Ends]);
        // Once.
        assert!(engine.tick((29 + ends_after + 5) * MINUTE).is_empty());
    }
}

/// A reload ends a held run whose skill is gone, whose trigger is gone or
/// moved to another topic, or that no longer holds until its trigger ends,
/// once; a skill reloaded with the same trigger keeps its hold (#1066's P3).
#[test]
fn a_reload_ends_a_held_run_whose_skill_or_trigger_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "stuffy", STUFFY);
    let high = br#"{"co2_ppm":1300}"#;
    let held = || {
        let mut engine = Engine::new(vec![load(&skill).unwrap()], true);
        engine.clock(0, 0, 600 * 60_000);
        let ran: usize = (0..30)
            .map(|minute| {
                engine
                    .observe("air/state", minute * MINUTE, high, None)
                    .len()
            })
            .sum();
        assert_eq!(ran, 1);
        engine
    };
    // The same skill again, and one edited with the same trigger: kept.
    let mut engine = held();
    assert!(engine.replace(vec![load(&skill).unwrap()]).is_empty());
    let louder = skill_with(
        &dir.path().join("louder"),
        "stuffy",
        &STUFFY.replace("dim = 1,", "dim = 2,"),
    );
    assert!(engine.replace(vec![load(&louder).unwrap()]).is_empty());
    // Its own end still comes.
    assert_eq!(engine.tick(44 * MINUTE)[0].calls, [Call::Ends]);
    let elsewhere = skill_with(
        &dir.path().join("elsewhere"),
        "stuffy",
        &STUFFY.replace("air/state", "air/indoor"),
    );
    let no_end = skill_with(
        &dir.path().join("no-end"),
        "stuffy",
        &STUFFY.replace(", until = \"ends\"", ""),
    );
    for after in [
        vec![],
        vec![load(&elsewhere).unwrap()],
        vec![load(&no_end).unwrap()],
    ] {
        let mut engine = held();
        let ended = engine.replace(after);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].skill, "stuffy");
        assert_eq!(ended[0].calls, [Call::Ends]);
        // Once: no second end from the reload or from the time after it.
        assert!(engine.replace(vec![]).is_empty());
        assert!(engine.tick(60 * MINUTE).is_empty());
    }
}

/// Task news (#1021) with `LAMP_SKILLS_ASK` not set, as the program answers
/// with neither the skip nor the Policy's gate (#958): each done is asked
/// about on its own, and nothing is done.
#[test]
fn task_news_the_policy_declines_does_nothing_and_asks_once_a_task() {
    let skill = load(&skills_root().join("task-news")).unwrap();
    let mut engine = Engine::new(vec![skill], false);
    let case = skills_root().join("task-news/tests/two-done-in-a-row");
    let mut runs = Vec::new();
    for message in read_recordings(&case).unwrap() {
        runs.extend(engine.observe(
            &message.header.topic,
            message.header.at_us,
            &message.payload,
            message.header.expires_ms,
        ));
    }
    // Tasks 4, 5, 6 and 8: four asks, four refusals, no call.
    assert_eq!(runs.len(), 4);
    for run in &runs {
        assert!(run.calls.is_empty());
        assert!(run.queue);
        assert_eq!(
            stages(&run.events),
            [
                SkillStage::Triggered,
                SkillStage::Asked,
                SkillStage::Answered
            ]
        );
        assert_eq!(run.events[1].why, "policy news");
        assert_eq!(run.events[2].allowed, Some(false));
        run.events.iter().for_each(|event| event.check().unwrap());
    }
    assert!(
        runs[0].events[0]
            .why
            .contains("task 4 of settled on tasks/status"),
        "{}",
        runs[0].events[0].why
    );
}

/// A list's items, each on its own, and steps by the trigger that fired.
const BY_ITEM: &str = r#"
[[trigger]]
name = "done"
topic = "tasks/status"
each = "tasks"
key = "task"
field = "state"
equals = "idle"
when = "appears"
[[trigger]]
name = "failed"
topic = "tasks/status"
each = "tasks"
key = "task"
field = "state"
equals = "failed"
when = "appears"
[ask]
policy = "news"
[[do]]
body = { gesture = "nod" }
[[do]]
on = "done"
say = { from = "tasks" }
[[do]]
on = "failed"
light = { dim = 1 }
say = "It failed."
"#;

/// A `tasks/status` with these tasks and states.
fn tasks(list: &[(u64, &str)]) -> String {
    let tasks: Vec<String> = list
        .iter()
        .map(|(task, state)| format!(r#"{{"task":{task},"engine":"claude","state":"{state}"}}"#))
        .collect();
    format!(r#"{{"tasks":[{}]}}"#, tasks.join(","))
}

#[test]
fn each_item_of_a_list_on_its_own_and_steps_by_the_trigger_that_fired() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "news", BY_ITEM);
    let (a, b, c, d, e) = (
        tasks(&[(1, "idle"), (2, "working")]),
        tasks(&[(1, "idle"), (2, "working"), (3, "working")]),
        tasks(&[(2, "idle"), (3, "failed")]),
        tasks(&[(2, "idle")]),
        tasks(&[(2, "working"), (3, "idle")]),
    );
    let runs = runs_at(
        &skill,
        &[
            (0, "tasks/status", &a),
            (10, "tasks/status", &b),
            (20, "tasks/status", &c),
            (30, "tasks/status", &d),
            (40, "tasks/status", &e),
            (50, "tasks/status", &d),
        ],
    );
    let nod = Call::Gesture("nod".into());
    // A say from what the voice keeps names the item's task (#1237).
    let done = |task| {
        vec![
            nod.clone(),
            Call::SayFrom(lamp_skills::Source::Tasks, Some(task)),
        ]
    };
    let failed = vec![
        nod.clone(),
        Call::Dim {
            steps: 1,
            until_ends: false,
        },
        Call::Say("It failed.".into()),
    ];
    // Task 1, idle at the start, is not news. At 20 s task 2 is done and
    // task 3 failed: two runs. At 40 s task 3, failed before and absent at
    // 30 s, is idle: done. At 50 s task 2 is idle again after working.
    assert_eq!(
        runs,
        [(20, done(2)), (20, failed), (40, done(3)), (50, done(2))]
    );
}

/// `tasks/status` with these tasks in its `settled` list (#1048).
fn settled(list: &[(u64, &str)]) -> String {
    let settled: Vec<String> = list
        .iter()
        .map(|(task, state)| {
            format!(
                r#"{{"task":{task},"engine":"claude","state":"{state}","settled":true,"at_unix_ms":1}}"#
            )
        })
        .collect();
    format!(r#"{{"tasks":[],"settled":[{}]}}"#, settled.join(","))
}

/// fourth on #1042: every item of #1048's `settled` is first seen already
/// settled, so an item new to the list counts; what is there at the first
/// message since the start, or since a reload, does not.
#[test]
fn each_item_of_a_list_comes_or_turns_and_a_restart_or_reload_is_not_news() {
    let dir = tempfile::tempdir().unwrap();
    let skill = load(&skill_with(
        dir.path(),
        "news",
        &BY_ITEM.replace("each = \"tasks\"", "each = \"settled\""),
    ))
    .unwrap();
    let mut engine = Engine::new(vec![skill.clone()], true);
    let mut at = 0;
    let mut send = |engine: &mut Engine, payload: &str| -> Vec<String> {
        at += 1_000_000;
        engine
            .observe("tasks/status", at, payload.as_bytes(), None)
            .into_iter()
            .map(|run| run.events[0].why.clone())
            .collect()
    };
    let none: Vec<String> = Vec::new();
    // The start: tasks 1 and 2 already settled are not news.
    assert_eq!(
        send(&mut engine, &settled(&[(1, "idle"), (2, "failed")])),
        none
    );
    assert_eq!(
        send(&mut engine, &settled(&[(1, "idle"), (2, "failed")])),
        none
    );
    // Task 3 comes in done, once; task 4 as needs_input is neither.
    let three = send(
        &mut engine,
        &settled(&[(1, "idle"), (2, "failed"), (3, "idle"), (4, "needs_input")]),
    );
    assert_eq!(three, ["task 3 of settled on tasks/status came to hold"]);
    assert_eq!(
        send(&mut engine, &settled(&[(3, "idle"), (4, "needs_input")])),
        none
    );
    // Task 4 turns to failed in the list: news.
    assert_eq!(
        send(&mut engine, &settled(&[(3, "idle"), (4, "failed")])).len(),
        1
    );
    // Task 3 leaves and comes back: news again.
    assert_eq!(send(&mut engine, &settled(&[(4, "failed")])), none);
    assert_eq!(
        send(&mut engine, &settled(&[(3, "idle"), (4, "failed")])).len(),
        1
    );
    // A reload: what was there at the reload (3, 4) is not news; the next
    // message is the next change, so task 5 in it is told (the reviewer on
    // #1042: a reload lost the next task's news), and so is 6 after it.
    engine.replace(vec![skill.clone()]);
    assert_eq!(
        send(
            &mut engine,
            &settled(&[(3, "idle"), (4, "failed"), (5, "idle")])
        ),
        ["task 5 of settled on tasks/status came to hold"]
    );
    assert_eq!(
        send(&mut engine, &settled(&[(5, "idle"), (6, "idle")])),
        ["task 6 of settled on tasks/status came to hold"]
    );
    // No list at all is an empty one.
    assert_eq!(send(&mut engine, r#"{"tasks":[]}"#), none);
    assert_eq!(send(&mut engine, &settled(&[(6, "idle")])).len(), 1);
    // A skill new at a reload, its topic kept already: what is there is not
    // news, and the next change is.
    let mut fresh = Engine::new(Vec::new(), true);
    assert_eq!(send(&mut fresh, &settled(&[(7, "idle")])), none);
    fresh.replace(vec![skill]);
    assert_eq!(
        send(&mut fresh, &settled(&[(7, "idle"), (8, "idle")])),
        ["task 8 of settled on tasks/status came to hold"]
    );
}

/// #1237: an item's `from = "tasks"` ask names its task, and a
/// `from = "schedule"` ask never does, since the voice refuses `task` on it.
#[test]
fn an_items_tasks_ask_names_its_task_and_a_schedule_ask_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let says = |from: &str| {
        let skill = load(&skill_with(
            &dir.path().join(from),
            "news",
            &BY_ITEM
                .replace("each = \"tasks\"", "each = \"settled\"")
                .replace(
                    "say = { from = \"tasks\" }",
                    &format!("say = {{ from = \"{from}\" }}"),
                ),
        ))
        .unwrap();
        let mut engine = Engine::new(vec![skill], true);
        engine.observe("tasks/status", 1, settled(&[]).as_bytes(), None);
        engine
            .observe("tasks/status", 2, settled(&[(7, "idle")]).as_bytes(), None)
            .into_iter()
            .flat_map(|run| run.calls)
            .filter(|call| matches!(call, Call::SayFrom(..)))
            .map(|call| call.named())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        says("tasks"),
        [("say", serde_json::json!({ "from": "tasks", "task": 7 }))]
    );
    assert_eq!(
        says("schedule"),
        [("say", serde_json::json!({ "from": "schedule" }))]
    );
}

/// The reviewer on #1042: item runs go in the items' order, by number when
/// the key is one, across the skill's triggers. Task 9 done and task 10
/// failed in one message are 9's run, then 10's; "10" sorted as text came
/// first.
#[test]
fn item_runs_go_by_the_items_number_across_triggers() {
    let dir = tempfile::tempdir().unwrap();
    let skill = load(&skill_with(
        dir.path(),
        "news",
        &BY_ITEM.replace("each = \"tasks\"", "each = \"settled\""),
    ))
    .unwrap();
    let froms = |first: &[(u64, &str)], then: &[(u64, &str)]| {
        let mut engine = Engine::new(vec![skill.clone()], true);
        engine.observe("tasks/status", 1, settled(first).as_bytes(), None);
        engine
            .observe("tasks/status", 2, settled(then).as_bytes(), None)
            .into_iter()
            .map(|run| {
                let said = run
                    .calls
                    .iter()
                    .any(|call| matches!(call, Call::Say(note) if note == "It failed."));
                (run.from.unwrap().1.unwrap(), said)
            })
            .collect::<Vec<_>>()
    };
    let nine = |failed: bool| ("9".to_owned(), failed);
    let ten = |failed: bool| ("10".to_owned(), failed);
    // 9 done, 10 failed: the done trigger's and the failed trigger's runs.
    assert_eq!(
        froms(&[], &[(9, "idle"), (10, "failed")]),
        [nine(false), ten(true)]
    );
    // 9 failed, 10 done: by number, not by trigger.
    assert_eq!(
        froms(&[], &[(9, "failed"), (10, "idle")]),
        [nine(true), ten(false)]
    );
    // Both done, from one trigger: 9 before 10, and before 100.
    let both = froms(&[], &[(9, "idle"), (10, "idle"), (100, "idle")]);
    let names: Vec<&str> = both.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["9", "10", "100"]);
}

/// With the Policy asked, an item's run asks as news, and its answer starts
/// it only while that item is still in the list and holds.
#[test]
fn an_items_run_asked_of_the_policy_goes_ahead_only_while_its_item_holds() {
    let dir = tempfile::tempdir().unwrap();
    let skill = load(&skill_with(dir.path(), "news", BY_ITEM)).unwrap();
    let mut engine = Engine::new(vec![skill], false).asking_the_policy();
    let status = |engine: &mut Engine, at_us: u64, list: &[(u64, &str)]| {
        engine.observe("tasks/status", at_us, tasks(list).as_bytes(), None)
    };
    assert!(status(&mut engine, 1, &[(1, "working"), (2, "working")]).is_empty());
    let runs = status(&mut engine, 2, &[(1, "idle"), (2, "working")]);
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert!(run.queue);
    assert_eq!(run.from, Some((0, Some("1".into()))));
    assert_eq!(
        run.ask.as_ref().unwrap(),
        &serde_json::json!({ "skill": "news", "act": "news" })
    );
    let done = vec![
        Call::Gesture("nod".into()),
        // Made again from the run's item: it still names task 1 (#1237).
        Call::SayFrom(lamp_skills::Source::Tasks, Some(1)),
    ];
    assert_eq!(engine.still(run, 3), Some(done));
    // Task 1 working again, or gone: nothing is done.
    status(&mut engine, 4, &[(1, "working"), (2, "working")]);
    assert_eq!(engine.still(run, 5), None);
    status(&mut engine, 6, &[(2, "idle")]);
    assert_eq!(engine.still(run, 7), None);
}

#[test]
fn the_loader_refuses_an_item_trigger_or_a_step_it_cannot_use() {
    let dir = tempfile::tempdir().unwrap();
    for (index, (from, to, why)) in [
        (
            "equals = \"idle\"\nwhen = \"appears\"",
            "equals = \"idle\"\nwhen = \"present\"",
            "each goes with",
        ),
        (
            "equals = \"idle\"\nwhen = \"appears\"",
            "equals = \"idle\"\nwhen = \"appears\"\ncooldown = \"1m\"",
            "each goes with",
        ),
        ("key = \"task\"\n", "", "each names a list"),
        (
            "name = \"failed\"",
            "name = \"done\"",
            "two triggers have one name",
        ),
        (
            "on = \"failed\"",
            "on = \"later\"",
            "names none of the skill's triggers",
        ),
        (
            "on = \"done\"\n",
            "on = \"done\"\nafter = \"say\"\n",
            "in a skill whose steps name their trigger",
        ),
        (
            "say = \"It failed.\"",
            "say = \"It failed.\"\n[[do]]\non = \"failed\"\nsay = \"Again.\"",
            "one say or sound a skill",
        ),
        (
            "body = { gesture = \"nod\" }",
            "say = \"Hello.\"",
            "one say or sound a skill",
        ),
        // Neither quiet hours nor a held dim on an item's run (the
        // reviewer on #1042, 09:56).
        (
            "[ask]",
            "[quiet]\nfrom = \"22:00\"\nto = \"08:00\"\n[ask]",
            "[quiet] in a skill with an each trigger",
        ),
        (
            "light = { dim = 1 }",
            "light = { dim = 1, until = \"ends\" }",
            "until = \"ends\" in a skill with an each trigger",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(BY_ITEM.contains(from), "{from}");
        let skill = skill_with(
            &dir.path().join(index.to_string()),
            "news",
            &BY_ITEM.replacen(from, to, 1),
        );
        let error = load(&skill).unwrap_err();
        assert!(error.contains(why), "{to}: {error}");
    }
    // One say for each trigger's run, as written.
    load(&skill_with(dir.path(), "news", BY_ITEM)).unwrap();
}

/// The program with an item skill on `tasks/status`'s list `each`, done on
/// `idle`, while the voice takes 600 ms over each say. Each value of
/// `values` is published, then waited after for its ms. Until `wanted` done
/// events: their whys, and each say asked, when.
fn the_program_with_items(
    each: &str,
    values: &[(String, u64)],
    wanted: usize,
) -> (Vec<String>, Vec<(Instant, String)>) {
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("sk")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    fs::create_dir(root.join("skills")).unwrap();
    let bus = Bus::new(root.join("bus")).unwrap();
    let mut events = bus.subscribe(SkillEvent::TOPIC, "test").unwrap();
    let voice = UnixListener::bind(root.join("say.sock")).unwrap();
    let (heard, asked) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in voice.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let _ = heard.send((Instant::now(), line));
            for answer in [r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#] {
                std::thread::sleep(Duration::from_millis(300));
                let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
            }
        }
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-skills"));
    command.args([
        "--dir".as_ref(),
        root.join("skills").as_os_str(),
        "--bus-root".as_ref(),
        root.join("bus").as_os_str(),
        "--say-socket".as_ref(),
        root.join("say.sock").as_os_str(),
    ]);
    command.env("LAMP_SKILLS_ASK", "skip");
    let _running = Running(command.spawn().unwrap());
    skill_with(
        &root.join("skills"),
        "news",
        &format!(
            "[trigger]\ntopic = \"tasks/status\"\neach = \"{each}\"\nkey = \"task\"\n\
             field = \"state\"\nequals = \"idle\"\nwhen = \"appears\"\n\
             [ask]\npolicy = \"news\"\n[[do]]\nsay = {{ from = \"tasks\" }}\n"
        ),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while bus.subscribers("tasks/status").unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the skill was never loaded");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut status = bus
        .publisher_declared(
            lamp_msgs::TaskStatus::TOPIC,
            &lamp_msgs::TaskStatus::declaration(),
            "tasks",
        )
        .unwrap();
    for (list, wait_ms) in values {
        let value: serde_json::Value = serde_json::from_str(list).unwrap();
        status.publish_json(1, &value).unwrap();
        std::thread::sleep(Duration::from_millis(*wait_ms));
    }
    let mut done = Vec::new();
    while done.len() < wanted && Instant::now() < deadline {
        if let Some(message) = events.recv(Duration::from_millis(100)).unwrap() {
            let event = message.parse::<SkillEvent>().unwrap();
            if event.stage == SkillStage::Done {
                done.push(event.why);
            }
        }
    }
    (done, asked.try_iter().collect())
}

/// The program with two tasks done in one message while the voice takes
/// 600 ms over each say: both are said, one after the other, neither let go.
#[test]
fn the_program_says_each_items_news_in_turn() {
    let (done, says) = the_program_with_items(
        "tasks",
        &[
            (tasks(&[(1, "working"), (2, "working")]), 100),
            (tasks(&[(1, "idle"), (2, "idle")]), 100),
        ],
        2,
    );
    assert_eq!(done, ["say: spoken", "say: spoken"]);
    assert_eq!(says.len(), 2, "{says:?}");
    assert!(
        says.iter()
            .all(|(_, line)| line.contains("\"from\":\"tasks\""))
    );
    // Each ask names its own task, in the tasks' order (#1237).
    assert!(says[0].1.contains("\"task\":1"), "{says:?}");
    assert!(says[1].1.contains("\"task\":2"), "{says:?}");
    // The second is asked only once the first was spoken.
    assert!(says[1].0 - says[0].0 >= Duration::from_millis(600));
}

/// The reviewer on #1042 (07:25): tasks 4, 5 and 6 settle in one message;
/// 5 leaves `settled` while 4's news is said. When 4's run is done, 5's is
/// let go, said so, and 6's still starts: the queue is not left waiting.
#[test]
fn a_queued_run_whose_item_left_is_let_go_and_the_next_still_starts() {
    let (done, says) = the_program_with_items(
        "settled",
        &[
            (settled(&[]), 100),
            (settled(&[(4, "idle"), (5, "idle"), (6, "idle")]), 200),
            (settled(&[(4, "idle"), (6, "idle")]), 100),
        ],
        3,
    );
    assert_eq!(
        done,
        [
            "say: spoken",
            "not made: its item no longer holds as its turn came",
            "say: spoken"
        ]
    );
    assert_eq!(says.len(), 2, "{says:?}");
    assert!(says[0].1.contains("\"task\":4"), "{says:?}");
    assert!(says[1].1.contains("\"task\":6"), "{says:?}");
    assert!(says[1].0 - says[0].0 >= Duration::from_millis(600));
}

/// How often Stuffy air droops from `unix_ms`, its clock told in `local`
/// (#1384). A droop's end, when the air clears, is not counted.
fn stuffy_droops_in(
    skill: &Path,
    air: &[(u64, u32)],
    local: &lamp_local_time::LocalTime,
    unix_ms: u64,
) -> usize {
    let mut engine = Engine::new(vec![load(skill).unwrap()], true);
    engine.clock_in(local, 0, unix_ms);
    air.iter()
        .map(|(minute, ppm)| {
            let payload = format!(r#"{{"co2_ppm":{ppm}}}"#);
            engine
                .observe("air/state", minute * 60_000_000, payload.as_bytes(), None)
                .iter()
                .filter(|run| run.calls.contains(&Call::Gesture("sleepy".into())))
                .count()
        })
        .sum()
}

/// #1384: a lamp in New York keeps its quiet hours (22:00 to 08:00) at the
/// same clock hours across 1 Nov 2026, when its clocks go back an hour. A
/// fixed offset from the day of setup (-240) starts the skill an hour early
/// all winter; the zone does not.
#[test]
fn quiet_hours_keep_their_clock_hours_across_summer_time_in_the_lamps_zone() {
    let dir = tempfile::tempdir().unwrap();
    let skill = skill_with(dir.path(), "stuffy", STUFFY);
    // Stuffy for half an hour, then clear: its tenth minute is 25 minutes
    // after the start, and it has cleared before 08:00 from 07:15.
    let mut air = co2(0, 30, 1300);
    air.extend(co2(30, 35, 800));
    let new_york = lamp_local_time::LocalTime::zone("America/New_York").unwrap();
    let setup_day = lamp_local_time::LocalTime::Offset(-240);
    // 31 Oct, 07:15 summer time (11:15 UTC): quiet either way.
    const SUMMER_0715: u64 = 1_793_445_300_000;
    assert_eq!(stuffy_droops_in(&skill, &air, &new_york, SUMMER_0715), 0);
    assert_eq!(stuffy_droops_in(&skill, &air, &setup_day, SUMMER_0715), 0);
    // 2 Nov, 07:15 winter time (12:15 UTC): still quiet in the zone, but
    // 08:15 by the fixed offset, so it would droop an hour early.
    const WINTER_0715: u64 = 1_793_621_700_000;
    assert_eq!(stuffy_droops_in(&skill, &air, &new_york, WINTER_0715), 0);
    assert_eq!(stuffy_droops_in(&skill, &air, &setup_day, WINTER_0715), 1);
    // 2 Nov, 08:15 winter time (13:15 UTC): past the night.
    const WINTER_0815: u64 = 1_793_625_300_000;
    assert_eq!(stuffy_droops_in(&skill, &air, &new_york, WINTER_0815), 1);
}

/// `lamp-skills` with `args`: its exit code and what it said, if it ended
/// within 5 s. One that starts serving instead is ended and fails the test.
fn refused(args: &[&str]) -> (Option<i32>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("lamp-skills {args:?} started instead of refusing");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// #1391's start path (the reviewer and Nine): the zone flags are refused
/// before anything starts when they can't stand, as memory refuses them.
#[test]
fn the_zone_flags_are_refused_before_the_start_when_they_cannot_stand() {
    for (args, said) in [
        (
            &["--zone", "system", "--utc-offset-minutes", "420"][..],
            "--zone and --utc-offset-minutes are one or the other",
        ),
        (
            &["--zone", "Not/A_Zone"][..],
            "time zone Not/A_Zone is unknown",
        ),
        (
            &["--zone", "system", "--zone", "Asia/Tokyo"][..],
            "--zone is given once",
        ),
        (
            &["--utc-offset-minutes", "420", "--utc-offset-minutes", "0"][..],
            "--utc-offset-minutes is given once",
        ),
        (
            &["--zone", ""][..],
            "--zone is system or a time zone's name",
        ),
        (
            &["--utc-offset-minutes", "900"][..],
            "--utc-offset-minutes is a whole number from -720 to 840",
        ),
    ] {
        let (code, stderr) = refused(args);
        assert_eq!(code, Some(2), "{args:?}: {stderr}");
        assert!(stderr.contains(said), "{args:?}: {stderr}");
    }
}

/// The tools' folder looked after under a running lamp-skills (lamp-4ace,
/// 19 Oct 04:33: systemd removed it with the actions part's at each of
/// actions' restarts). Its socket gone from a folder still there: at the
/// next look the socket and the file are made again, and a call on the new
/// socket is answered. The folder itself gone, here under a parent it
/// cannot write, as `/run` is to it on a lamp: it ends, not zero, with a
/// line, so that systemd makes the folder again as it starts it (the
/// reviewer and the first builder on #1656).
#[test]
fn the_tools_socket_lost_is_made_again_and_the_folder_gone_ends_lamp_skills() {
    use std::os::unix::fs::PermissionsExt;
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("st")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    fs::create_dir(root.join("skills")).unwrap();
    skill_with(&root.join("skills"), "tutor", TUTOR);
    let run = root.join("run");
    fs::create_dir(&run).unwrap();
    let tools = run.join("tools");
    let clock = Clock::at(root);
    let mut child = Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
        .args([
            "--dir".as_ref(),
            root.join("skills").as_os_str(),
            "--bus-root".as_ref(),
            root.join("bus").as_os_str(),
            "--tools-dir".as_ref(),
            tools.as_os_str(),
        ])
        .args(clock.args())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let made = |name: &str| {
        while !tools.join(name).exists() {
            assert!(Instant::now() < deadline, "no {name}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    made("call.sock");
    clock.started();
    made("tools.json");
    let file = fs::read_to_string(tools.join("tools.json")).unwrap();
    // The socket lost, the folder still there.
    fs::remove_file(tools.join("call.sock")).unwrap();
    fs::remove_file(tools.join("tools.json")).unwrap();
    clock.skip(RELOAD);
    made("call.sock");
    made("tools.json");
    assert_eq!(fs::read_to_string(tools.join("tools.json")).unwrap(), file);
    let stream = UnixStream::connect(tools.join("call.sock")).unwrap();
    writeln!(
        &stream,
        r#"{{"op":"call","name":"nobody_has_this","args":{{}}}}"#
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).unwrap();
    let answer: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(answer["ok"], false, "{answer}");
    // The folder gone, under a parent it cannot write.
    fs::remove_dir_all(&tools).unwrap();
    fs::set_permissions(&run, fs::Permissions::from_mode(0o555)).unwrap();
    clock.skip(RELOAD);
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("lamp-skills went on with its folder gone");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    fs::set_permissions(&run, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!status.success(), "{status}");
    let mut said = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut said).unwrap();
    assert!(said.contains("is gone; ending"), "{said}");
    assert!(!tools.exists(), "not made again by lamp-skills");
}

/// A program on a bus of its own whose skills a test calls by their tools,
/// with the body's mood socket, the actions part (which says each colour on
/// `light/state` at 3, as the real one shows it) and the voice's state
/// stood in for, on a clock the test skips (#1679).
struct ToolProgram {
    _dir: tempfile::TempDir,
    tools: PathBuf,
    clock: Clock,
    mood: UnixDatagram,
    /// Each call to the actions part, in order.
    lights: mpsc::Receiver<String>,
    voice: lamp_bus::Publisher,
    _running: Running,
}

impl ToolProgram {
    /// With the skill `name` of `toml` and its `shows/`, the light shown
    /// blue at 3, ready for a call.
    fn with(name: &str, toml: &str, shows: &[(&str, &str)]) -> Self {
        // Short: a socket's whole path has to fit in 103 bytes.
        let dir = tempfile::Builder::new()
            .prefix("tp")
            .tempdir_in("/tmp")
            .unwrap();
        let root = dir.path().to_owned();
        fs::create_dir(root.join("skills")).unwrap();
        let bus = Bus::new(root.join("bus")).unwrap();
        let actions = UnixListener::bind(root.join("actions.sock")).unwrap();
        let (called, lights) = mpsc::channel();
        let shown = bus.clone();
        std::thread::spawn(move || {
            let mut shown = shown
                .publisher_declared("light/state", &Light::declaration(), "ring")
                .unwrap();
            for stream in actions.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let call: serde_json::Value = serde_json::from_str(&line).unwrap();
                if let Some(colour) = call["args"]["colour"].as_str() {
                    shown
                        .publish_json(1, &Light::new(true, colour, 3).unwrap())
                        .unwrap();
                }
                // Kept before it is answered: a call answered was seen.
                let _ = called.send(line);
                (&stream)
                    .write_all(b"{\"ok\":true,\"result\":{}}\n")
                    .unwrap();
            }
        });
        let mut ring = bus
            .publisher_declared("light/state", &Light::declaration(), "ring")
            .unwrap();
        ring.publish_json(1, &Light::new(true, "blue", 3).unwrap())
            .unwrap();
        let voice = bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        let mood = UnixDatagram::bind(root.join("mood.sock")).unwrap();
        mood.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let tools = root.join("tools");
        let clock = Clock::at(&root);
        let running = Running(
            Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
                .args([
                    "--dir".as_ref(),
                    root.join("skills").as_os_str(),
                    "--bus-root".as_ref(),
                    root.join("bus").as_os_str(),
                    "--mood-socket".as_ref(),
                    root.join("mood.sock").as_os_str(),
                    "--actions-socket".as_ref(),
                    root.join("actions.sock").as_os_str(),
                    "--tools-dir".as_ref(),
                    tools.as_os_str(),
                ])
                .args(clock.args())
                .spawn()
                .unwrap(),
        );
        let skill = skill_with(&root.join("skills"), name, toml);
        if !shows.is_empty() {
            fs::create_dir(skill.join("shows")).unwrap();
            for (show, text) in shows {
                fs::write(skill.join("shows").join(format!("{show}.toml")), text).unwrap();
            }
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !tools.join("tools.json").exists() {
            assert!(Instant::now() < deadline, "no tools file");
            std::thread::sleep(Duration::from_millis(50));
        }
        Self {
            _dir: dir,
            tools,
            clock,
            mood,
            lights,
            voice,
            _running: running,
        }
    }

    /// The model's call of `name` with `args`, as the voice makes it.
    fn call(&self, name: &str, args: &str) -> serde_json::Value {
        let stream = UnixStream::connect(self.tools.join("call.sock")).unwrap();
        writeln!(&stream, r#"{{"op":"call","name":"{name}","args":{args}}}"#).unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(&line).unwrap()
    }

    /// What the mood socket was told over `window` of the program's clock,
    /// a second on at a time, each second's said waited for.
    fn poses(&self, window: Duration) -> Vec<String> {
        let until = self.clock.now() + window;
        let mut said = self.said();
        while self.clock.now() < until {
            self.clock
                .skip_to((self.clock.now() + Duration::from_secs(1)).min(until));
            said.extend(self.said());
        }
        said
    }

    /// What the mood socket holds, once the program's threads have had a
    /// moment to say what is due.
    fn said(&self) -> Vec<String> {
        std::thread::sleep(Duration::from_millis(100));
        self.mood.set_nonblocking(true).unwrap();
        let mut buffer = [0u8; 256];
        let mut said = Vec::new();
        while let Ok(length) = self.mood.recv(&mut buffer) {
            said.push(String::from_utf8_lossy(&buffer[..length]).into_owned());
        }
        self.mood.set_nonblocking(false).unwrap();
        said
    }

    /// The colours the actions part was asked for since the last look,
    /// waiting up to five seconds for `least` of them.
    fn colours(&self, least: usize) -> Vec<String> {
        self.colours_within(least, Duration::from_secs(5))
    }

    /// The same, waiting at most `within`.
    fn colours_within(&self, least: usize, within: Duration) -> Vec<String> {
        let colour = |line: String| {
            let call: serde_json::Value = serde_json::from_str(&line).ok()?;
            call["args"]["colour"].as_str().map(str::to_owned)
        };
        let deadline = Instant::now() + within;
        let mut colours = Vec::new();
        while colours.len() < least && Instant::now() < deadline {
            if let Ok(line) = self.lights.recv_timeout(Duration::from_millis(50)) {
                colours.extend(colour(line));
            }
        }
        // And whatever else came with them.
        std::thread::sleep(Duration::from_millis(100));
        colours.extend(self.lights.try_iter().filter_map(colour));
        colours
    }
}

/// Two scenes, the first with the reading pose (#1679).
const SCENES: &str = r#"
tool = "light_scene"
[choice]
name = "scene"
about = "The scene."
one_of = ["reading", "relax"]
[[do]]
say = "Say it is set."
[[do]]
if_choice = ["reading"]
body = { mood = "reading", for = "5s" }
light = { colour = "white", keep = true }
[[do]]
if_choice = ["relax"]
light = { colour = "warm", keep = true }
"#;

/// #1679: a scene's light is the person's, kept: set through the actions
/// part and never given back at the run's end. Another scene asked while
/// the first's pose is held sets its light at once and lets the pose go,
/// so the head comes back up rather than staying down over the desk.
#[test]
fn a_scene_keeps_the_persons_light_and_the_next_scene_lets_the_pose_go() {
    let program = ToolProgram::with("scenes", SCENES, &[]);
    let reading = program.call("light_scene", r#"{"scene":"reading"}"#);
    assert_eq!(reading["ok"], true, "{reading}");
    assert_eq!(reading["result"]["next"], "Say it is set.");
    assert_eq!(program.colours(1), ["white"]);
    let held = program.poses(Duration::from_secs(2));
    assert!(
        held.len() >= 2
            && held
                .iter()
                .all(|pose| pose == r#"{"level":0,"mood":"reading"}"#),
        "{held:?}"
    );
    let relax = program.call("light_scene", r#"{"scene":"relax"}"#);
    assert_eq!(relax["ok"], true, "{relax}");
    // The kept light is made after the hold is let go: once it is seen,
    // what the hold said before that is all there is of it.
    assert_eq!(program.colours(1), ["warm"]);
    let _before = program.said();
    // The pose let go: nothing more said, though its five seconds last.
    assert_eq!(program.poses(Duration::from_secs(5)), Vec::<String>::new());
    // Past the run's end, nothing given back: the warm light stays theirs.
    program.clock.skip(Duration::from_secs(2));
    assert_eq!(program.colours(0), Vec::<String>::new());
}

/// The review on #1730: a pose held again by a call while its run was under
/// way lasts its own `for`, past the run's end. A scene asked after the run
/// ended, with no pose of its own, still lets that hold go.
#[test]
fn a_scene_after_the_run_ended_still_lets_a_pose_held_again_go() {
    let program = ToolProgram::with("scenes-after", SCENES, &[]);
    assert_eq!(
        program.call("light_scene", r#"{"scene":"reading"}"#)["ok"],
        true
    );
    assert_eq!(program.colours(1), ["white"]);
    let _ = program.poses(Duration::from_secs(2));
    // Again at 2 s, under way: its pose held again, for 5 s from now.
    assert_eq!(
        program.call("light_scene", r#"{"scene":"reading"}"#)["ok"],
        true
    );
    let _ = program.colours(1);
    // Past the first run's end (5 s), inside the second hold (7 s).
    let held = program.poses(Duration::from_millis(3_800));
    assert!(!held.is_empty(), "the hold said again: {held:?}");
    assert_eq!(
        program.call("light_scene", r#"{"scene":"relax"}"#)["ok"],
        true
    );
    assert_eq!(program.colours(1), ["warm"]);
    let _before = program.said();
    assert_eq!(program.poses(Duration::from_secs(3)), Vec::<String>::new());
}

/// #1679: a call again that turns the pose toward another place takes the
/// place of the hold under way: from then on only the turned pose is said,
/// never the two in turn.
#[test]
fn a_turned_pose_takes_the_place_of_the_one_held() {
    let point = r#"
tool = "point_light"
[choice]
name = "toward"
about = "Where."
one_of = ["ahead", "left"]
default = "ahead"
[[do]]
body = { mood = "reading", for = "10s" }
say = "Say whether you can see it."
[[do]]
if_choice = ["left"]
body = { mood = "reading", for = "10s", toward = -0.5 }
"#;
    let program = ToolProgram::with("point", point, &[]);
    assert_eq!(program.call("point_light", "{}")["ok"], true);
    let plain = r#"{"level":0,"mood":"reading"}"#;
    let turned = r#"{"level":0,"mood":"reading","toward":-0.5}"#;
    let before = program.poses(Duration::from_secs(2));
    assert!(
        before.len() >= 2 && before.iter().all(|pose| pose == plain),
        "{before:?}"
    );
    assert_eq!(
        program.call("point_light", r#"{"toward":"left"}"#)["ok"],
        true
    );
    let after = program.poses(Duration::from_secs(4));
    // The call again says each of its poses once (the plain one first),
    // then only the turned one is held.
    let held: Vec<&String> = after.iter().skip_while(|pose| *pose != turned).collect();
    assert!(
        held.len() >= 4 && held.iter().all(|pose| *pose == turned),
        "{after:?}"
    );
}

/// A short show for the program's tests: three frames of two seconds.
const SHORT_SHOW: &str = r#"
[[light]]
colour = "purple"
level = 3
for = "2s"
[[light]]
colour = "pink"
level = 3
for = "2s"
[[light]]
colour = "green"
level = 3
for = "2s"
"#;

/// #1679: a light show plays its frames in turn, each its while, and gives
/// the light back after; someone speaking (the voice listening) stops it
/// within a fifth of a second, and the light is given back then.
#[test]
fn a_light_show_plays_its_frames_in_turn_and_stops_when_someone_speaks() {
    let show = r#"
tool = "light_show"
[[do]]
say = "Say the show is starting."
[[do]]
after = "say"
show = "short"
"#;
    let mut program = ToolProgram::with("show", show, &[("short", SHORT_SHOW)]);
    assert_eq!(program.call("light_show", "{}")["ok"], true);
    // No voice state said: the steps after the line go on at once.
    assert_eq!(program.colours(1), ["purple"]);
    program.clock.skip(Duration::from_millis(2_100));
    assert_eq!(program.colours(1), ["pink"]);
    program.clock.skip(Duration::from_millis(2_100));
    assert_eq!(program.colours(1), ["green"]);
    program.clock.skip(Duration::from_millis(2_100));
    // Given back as it was: blue.
    assert_eq!(program.colours(1), ["blue"]);
    // Again, and someone speaks during its first frame: the next frame is
    // never shown, and the light is given back at once.
    assert_eq!(program.call("light_show", "{}")["ok"], true);
    assert_eq!(program.colours(1), ["purple"]);
    program
        .voice
        .publish_json(
            1,
            &VoiceState {
                state: lamp_msgs::Voice::Listening,
            },
        )
        .unwrap();
    program.clock.skip(Duration::from_millis(400));
    // Within a fifth of a second of the program's clock, well before the
    // frame's two seconds end: a look at the frame's end alone is too late.
    assert_eq!(
        program.colours_within(1, Duration::from_millis(600)),
        ["blue"]
    );
    program.clock.skip(Duration::from_secs(5));
    assert_eq!(program.colours(0), Vec::<String>::new());
}

/// #1679: the loader holds the new steps to what they are for: a kept
/// light and a show only in a skill a person starts by its tool; a kept
/// light steady and for good; a turn only of the reading pose, across the
/// picture; a step for some choices only of the skill's own values, and no
/// say in one; a show's frames within their counts and whiles; a choice's
/// default one of its values.
#[test]
fn the_loader_holds_kept_lights_shows_turns_and_choices_to_their_rules() {
    let dir = tempfile::tempdir().unwrap();
    const TOOL: &str =
        "tool = \"t\"\n[choice]\nname = \"c\"\nabout = \"C.\"\none_of = [\"a\", \"b\"]\n";
    const TRIGGERED: &str = "[trigger]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n[ask]\npolicy = \"hello\"\n";
    for (number, (toml, why)) in [
        (
            format!("{TRIGGERED}[[do]]\nlight = {{ colour = \"warm\", keep = true }}\n"),
            "only a light a person asked for",
        ),
        (
            format!("{TOOL}[[do]]\nlight = {{ colour = \"warm\", keep = true, for = \"5s\" }}\n"),
            "no for and no pattern",
        ),
        (
            format!("{TOOL}[[do]]\nlight = {{ dim = 1, keep = true }}\n"),
            "keep goes with a colour",
        ),
        (
            format!("{TOOL}[[do]]\nbody = {{ toward = 0.5 }}\n"),
            "goes with mood = \"reading\"",
        ),
        (
            format!("{TOOL}[[do]]\nbody = {{ mood = \"reading\", for = \"5s\", toward = 1.5 }}\n"),
            "-1 to 1",
        ),
        (
            format!("{TOOL}[[do]]\nif_choice = [\"z\"]\nlight = {{ colour = \"warm\" }}\n"),
            "values of the choice",
        ),
        (
            format!("{TOOL}[[do]]\nif_choice = [\"a\", \"a\"]\nlight = {{ colour = \"warm\" }}\n"),
            "each once",
        ),
        (
            format!("{TOOL}[[do]]\nif_choice = [\"a\"]\nsay = \"Hi.\"\n"),
            "body, light or show",
        ),
        (
            "tool = \"t\"\n[[do]]\nif_choice = [\"a\"]\nlight = { colour = \"warm\" }\n".into(),
            "needs a [choice]",
        ),
        (
            format!("{TRIGGERED}[[do]]\nshow = \"short\"\n"),
            "only when a person asks",
        ),
        (
            "tool = \"t\"\n[[do]]\nlight = { colour = \"warm\", keep = true }\n[[do]]\nlight = { colour = \"blue\", level = 3, for = \"5s\" }\n".into(),
            "goes with no light given back",
        ),
        (
            "tool = \"t\"\n[choice]\nname = \"c\"\nabout = \"C.\"\none_of = [\"a\"]\ndefault = \"z\"\n[[do]]\nsay = \"Hi.\"\n".into(),
            "its default",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let error = load(&skill_with(dir.path(), &format!("bad{number}"), &toml)).unwrap_err();
        assert!(error.contains(why), "{toml}: {error}");
    }
    // A show's frames: 2 to 30, each 2 s to 20 s, a minute at most, the
    // ring's patterns only.
    let frame = |colour: &str, lasting: &str| {
        format!("[[light]]\ncolour = \"{colour}\"\nlevel = 3\nfor = \"{lasting}\"\n")
    };
    for (number, (show, why)) in [
        (frame("pink", "2s"), "2 to 30"),
        (frame("pink", "2s") + &frame("pink", "1s"), "2s to 20s"),
        (frame("pink", "20s").repeat(4), "a minute at most"),
        (frame("pink", "2s") + &frame("magenta", "2s"), "is not one of"),
        (
            frame("pink", "4s")
                + "[[light]]\ncolour = \"pink\"\nlevel = 3\nfor = \"4s\"\npattern = { kind = \"strobe\" }\n",
            "no blink, flash or rainbow",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let skill = skill_with(
            dir.path(),
            &format!("show{number}"),
            "tool = \"t\"\n[[do]]\nshow = \"short\"\n",
        );
        fs::create_dir_all(skill.join("shows")).unwrap();
        fs::write(skill.join("shows/short.toml"), show).unwrap();
        let error = load(&skill).unwrap_err();
        assert!(error.contains(why), "{error}");
    }
    let skill = skill_with(
        dir.path(),
        "good",
        "tool = \"t\"\n[[do]]\nshow = \"short\"\n",
    );
    fs::create_dir_all(skill.join("shows")).unwrap();
    fs::write(skill.join("shows/short.toml"), SHORT_SHOW).unwrap();
    load(&skill).unwrap();
}

/// #1679: a step for some choices is made only in a call that chose one of
/// them, and a choice's default stands for a call that gives none.
#[test]
fn a_step_for_some_choices_is_made_only_for_them_and_the_default_stands_in() {
    let dir = tempfile::tempdir().unwrap();
    let skill = load(&skill_with(
        dir.path(),
        "scenes",
        &SCENES.replace(
            "one_of = [\"reading\", \"relax\"]",
            "one_of = [\"reading\", \"relax\"]\ndefault = \"relax\"",
        ),
    ))
    .unwrap();
    let mut engine = Engine::new(vec![skill], true);
    let calls =
        |engine: &mut Engine, args: serde_json::Value| -> Vec<(&'static str, serde_json::Value)> {
            engine
                .tool("light_scene", &args, 1_000_000)
                .unwrap()
                .calls
                .iter()
                .map(Call::named)
                .collect()
        };
    let reading = calls(&mut engine, serde_json::json!({"scene": "reading"}));
    assert_eq!(
        reading.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["say", "body.mood", "light"]
    );
    assert_eq!(reading[2].1["colour"], "white");
    assert_eq!(reading[2].1["keep"], true);
    for args in [serde_json::json!({"scene": "relax"}), serde_json::json!({})] {
        let relax = calls(&mut engine, args);
        assert_eq!(
            relax.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            ["say", "light"]
        );
        assert_eq!(relax[1].1["colour"], "warm");
    }
}

/// The first tier of #1684's design as a trigger: PM2.5 up at least 100
/// within 3 minutes, to above 150, both for 30 s through a 10 s gap; once a
/// spell, again only after 10 minutes under 150, and `cooldown` (30 minutes
/// in the design) when given.
fn smoky(dir: &Path, name: &str, cooldown: &str) -> PathBuf {
    skill_with(
        dir,
        name,
        &format!(
            r#"
needs = ["air", "voice"]
[trigger]
topic = "air/state"
field = "pm2_5"
above = 150
rise = {{ by = 100, within = "3m" }}
for = "30s"
gap = "10s"
rearm = {{ below = 150, for = "10m" }}
{cooldown}
[ask]
policy = "nudge"
[[do]]
say = {{ text = "The air near me just got smoky. If something's burning, please check it.", priority = 8, expires = "2m" }}
"#
        ),
    )
}

/// A reading of `air/state` every 5 s from `from` up to `to` seconds, its
/// PM2.5 what `pm` gives at each second: a number, or `None` for a reading
/// the sensor does not know (`null`, as `lamp-air` sends it).
fn air_from(
    from: u64,
    to: u64,
    pm: impl Fn(u64) -> Option<f64>,
) -> Vec<(u64, &'static str, String)> {
    (from..to)
        .step_by(5)
        .map(|at| {
            let pm = pm(at).map_or("null".to_owned(), |pm| pm.to_string());
            (
                at,
                "air/state",
                format!(r#"{{"pm2_5":{pm},"feel":"fine"}}"#),
            )
        })
        .collect()
}

/// The seconds at which `skill` ran on `readings`.
fn air_runs(skill: &Path, readings: &[(u64, &'static str, String)]) -> Vec<u64> {
    let messages: Vec<(u64, &str, &str)> = readings
        .iter()
        .map(|(at, topic, payload)| (*at, *topic, payload.as_str()))
        .collect();
    runs_at(skill, &messages)
        .into_iter()
        .map(|(at, _)| at)
        .collect()
}

/// #1684 (a): a rise holds where a level alone can't tell smoke from a
/// city's air. A sharp rise speaks once, after its hold; a slow climb to the
/// same level never; noise around the level and a short puff never.
#[test]
fn a_rise_fires_on_a_sharp_climb_and_never_on_a_slow_one_or_noise() {
    let dir = tempfile::tempdir().unwrap();
    let skill = smoky(dir.path(), "smoky-air", "cooldown = \"30m\"");
    // 80 for 5 minutes, then 400: the rise holds from 300 s, and its 30 s
    // hold is over at 330 s. Once, though it stays smoky for 15 minutes.
    let sharp = air_from(0, 1_200, |at| Some(if at < 300 { 80.0 } else { 400.0 }));
    assert_eq!(air_runs(&skill, &sharp), [330]);
    // The design's first lamp case: 80, then up to 400 within 90 s. It
    // passes 180 (100 over 80) at 330 s, and its hold ends at 360 s.
    let ramp = air_from(0, 900, |at| {
        Some(match at {
            ..300 => 80.0,
            300..390 => 80.0 + 320.0 * (at - 300) as f64 / 90.0,
            _ => 400.0,
        })
    });
    assert_eq!(air_runs(&skill, &ramp), [360]);
    // The design's slow case: 80 to 300 over 30 minutes is 22 in any 3
    // minutes. It passes the level and stays there, and nothing is said.
    let slow = air_from(0, 3_600, |at| {
        Some(80.0 + 220.0 * (at.min(1_800)) as f64 / 1_800.0)
    });
    assert!(air_runs(&skill, &slow).is_empty());
    // Haze already above the level, noisy by 40 between readings: no rise.
    let noisy = air_from(0, 3_600, |at| {
        Some(if at % 10 == 0 { 130.0 } else { 170.0 })
    });
    assert!(air_runs(&skill, &noisy).is_empty());
    // A match struck by the base: 300 for 15 s in an office's 100, three
    // times 5 minutes apart. Each rises past both lines, none for 30 s.
    let puffs = air_from(0, 1_200, |at| {
        Some(if at >= 300 && at % 300 < 15 {
            300.0
        } else {
            100.0
        })
    });
    assert!(air_runs(&skill, &puffs).is_empty());
    // A rise of 100 that stays under the level, 40 to 140: no smoke.
    let under = air_from(0, 900, |at| Some(if at < 300 { 40.0 } else { 140.0 }));
    assert!(air_runs(&skill, &under).is_empty());
}

/// #1684 (a): a rise needs readings in the while before it. A sensor that
/// was silent, or knew no PM2.5, for longer than the while gives nothing to
/// compare its first readings with, so coming back high is no rise; a gap
/// shorter than the while still is one.
#[test]
fn a_rise_after_a_gap_in_readings_or_a_dropout_is_not_one() {
    let dir = tempfile::tempdir().unwrap();
    let skill = smoky(dir.path(), "smoky-air", "cooldown = \"30m\"");
    // 80, then nothing for 5 minutes, then 400 for 10.
    let mut silent = air_from(0, 300, |_| Some(80.0));
    silent.extend(air_from(600, 1_200, |_| Some(400.0)));
    assert!(air_runs(&skill, &silent).is_empty());
    // 80, then 5 minutes of readings with no PM2.5 (null), then 400.
    let unknown = air_from(0, 1_200, |at| match at {
        ..300 => Some(80.0),
        300..600 => None,
        _ => Some(400.0),
    });
    assert!(air_runs(&skill, &unknown).is_empty());
    // A reading with no PM2.5 in the middle of a rise is passed over, and
    // neither breaks its hold nor counts as one: the rise at 300 s still
    // speaks at 330 s, its hold counted through the dropouts.
    let dropouts = air_from(0, 900, |at| match at {
        ..300 => Some(80.0),
        310 | 320 => None,
        _ => Some(400.0),
    });
    assert_eq!(air_runs(&skill, &dropouts), [330]);
    // Silent for a minute, under the 3-minute while: 80 is still in it,
    // and the rise is a rise.
    let mut short = air_from(0, 300, |_| Some(80.0));
    short.extend(air_from(360, 900, |_| Some(400.0)));
    assert_eq!(air_runs(&skill, &short), [390]);
}

/// #1684 (a): once a spell. A second rise before the field has stayed under
/// 150 for 10 minutes is not spoken, however sharp; one after is, unless
/// the cooldown still holds.
#[test]
fn a_rise_rearms_only_after_its_line_and_its_cooldown() {
    let dir = tempfile::tempdir().unwrap();
    let pm = |at: u64| {
        Some(match at {
            ..300 => 80.0,
            300..600 => 400.0,
            // 5 minutes under 150: not enough.
            600..900 => 100.0,
            900..1_000 => 400.0,
            // 11 minutes and more under 150: armed again at 1,600 s.
            1_000..1_700 => 100.0,
            _ => 400.0,
        })
    };
    let readings = air_from(0, 2_400, pm);
    let without = smoky(dir.path(), "smoky-air", "");
    assert_eq!(air_runs(&without, &readings), [330, 1_730]);
    // The design's 30 minutes from the first: the third rise is in them.
    let cooled = smoky(
        &dir.path().join("cooled"),
        "smoky-air",
        "cooldown = \"30m\"",
    );
    assert_eq!(air_runs(&cooled, &readings), [330]);
    // The rise at 1,700 s, held back by the cooldown, is spent as one that
    // ran: the line is waited for again, and the next rise after it speaks.
    let later = air_from(0, 3_000, |at| match at {
        ..1_800 => pm(at),
        1_800..2_500 => Some(100.0),
        _ => Some(400.0),
    });
    assert_eq!(air_runs(&cooled, &later), [330, 2_530]);
}

/// #1684 (a): the readings a rise compares with begin again at a reload,
/// and the Policy's late answer finds the rise as its newest reading had it.
#[test]
fn a_rise_begins_again_at_a_reload_and_is_asked_again_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let skill = smoky(dir.path(), "smoky-air", "cooldown = \"30m\"");
    let air = |pm: f64| format!(r#"{{"pm2_5":{pm},"feel":"fine"}}"#);
    let mut engine = Engine::new(vec![load(&skill).unwrap()], true);
    for at in (0..300).step_by(5) {
        assert!(
            engine
                .observe("air/state", at * 1_000_000, air(80.0).as_bytes(), None)
                .is_empty()
        );
    }
    assert!(engine.replace(vec![load(&skill).unwrap()]).is_empty());
    for at in (300..900).step_by(5) {
        let runs = engine.observe("air/state", at * 1_000_000, air(400.0).as_bytes(), None);
        assert!(runs.is_empty(), "at {at} s: {runs:?}");
    }
    let mut engine = Engine::new(vec![load(&skill).unwrap()], false).asking_the_policy();
    let mut asked = None;
    for at in (0..335).step_by(5) {
        let pm = if at < 300 { 80.0 } else { 400.0 };
        asked = asked.or(engine
            .observe("air/state", at * 1_000_000, air(pm).as_bytes(), None)
            .pop());
    }
    let run = asked.expect("asked at 330 s");
    assert!(run.ask.is_some());
    assert!(engine.still(&run, 331_000_000).is_some());
    // Three minutes of 400 later the rise is over, though the level holds:
    // an answer that came only then would say nothing.
    for at in (335..520).step_by(5) {
        engine.observe("air/state", at * 1_000_000, air(400.0).as_bytes(), None);
    }
    assert_eq!(engine.still(&run, 520_000_000), None);
}

/// #1684 (b) and (c): a say written as a table is the skill's own words as
/// written (`text`) or a note (`note`), at a priority up to 8 and spent after
/// its `expires`; what it can't be is refused with why.
#[test]
fn a_say_table_is_exact_words_or_a_note_at_its_priority() {
    let dir = tempfile::tempdir().unwrap();
    let skill = smoky(dir.path(), "smoky-air", "");
    let mut engine = Engine::new(vec![load(&skill).unwrap()], true);
    let mut calls = Vec::new();
    for at in (0..335).step_by(5) {
        let pm = if at < 300 { 80 } else { 400 };
        let payload = format!(r#"{{"pm2_5":{pm}}}"#);
        for run in engine.observe("air/state", at * 1_000_000, payload.as_bytes(), None) {
            calls.extend(run.calls);
        }
    }
    assert_eq!(calls.len(), 1, "{calls:?}");
    let (name, args) = calls[0].named();
    assert_eq!(name, "say");
    assert_eq!(
        args,
        serde_json::json!({
            "text": "The air near me just got smoky. If something's burning, please check it.",
            "priority": 8,
            "expires_ms": 120_000,
        })
    );
    // A note at its own priority, and the defaults: 5 and 30 s.
    let note = skill_with(
        dir.path(),
        "noted",
        "[trigger]\ntopic = \"humans/faces/tracked\"\nwhen = \"appears\"\n[ask]\npolicy = \"hello\"\n\
         [[do]]\nsay = { note = \"Greet them.\" }\n",
    );
    let runs = runs_at(&note, &[(0, Faces::TOPIC, NOBODY), (5, Faces::TOPIC, FACE)]);
    assert_eq!(
        runs[0].1[0].named().1,
        serde_json::json!({"note": "Greet them.", "priority": 5, "expires_ms": 30_000})
    );
    for (say, why) in [
        (
            "{ text = \"Smoke.\", priority = 9 }",
            "0 to 8: 9 is the voice's own",
        ),
        ("{ text = \"Smoke.\", priority = -1 }", "0 to 8"),
        (
            "{ text = \"Smoke.\", note = \"Smoke.\" }",
            "one of text or note",
        ),
        ("{ priority = 8 }", "one of text or note"),
        (
            "{ text = \"Smoke.\", loud = true }",
            "say.loud is not known",
        ),
        ("{ text = \"\" }", "1 to 900 bytes"),
        ("{ text = \"Smoke.\\nFire.\" }", "no control characters"),
        ("{ text = 5 }", "1 to 900 bytes"),
        ("{ text = \"Smoke.\", expires = \"500ms\" }", "1s to 1h"),
        ("{ text = \"Smoke.\", expires = \"2h\" }", "1s to 1h"),
        ("{ text = \"Smoke.\", expires = 120 }", "a duration"),
        (
            "{ from = \"schedule\", priority = 9 }",
            "from and nothing else",
        ),
    ] {
        let skill = skill_with(
            dir.path(),
            "refused",
            &format!(
                "[trigger]\ntopic = \"humans/faces/tracked\"\n[ask]\npolicy = \"hello\"\n[[do]]\nsay = {say}\n"
            ),
        );
        let error = load(&skill).unwrap_err();
        assert!(error.contains(why), "{say}: {error}");
    }
    let long = format!("{{ text = \"{}\" }}", "a".repeat(901));
    let skill = skill_with(
        dir.path(),
        "refused",
        &format!(
            "[trigger]\ntopic = \"humans/faces/tracked\"\n[ask]\npolicy = \"hello\"\n[[do]]\nsay = {long}\n"
        ),
    );
    assert!(load(&skill).unwrap_err().contains("1 to 900 bytes"));
    // A tool's say is the note for the model's answer, in its own turn.
    let tool = skill_with(
        dir.path(),
        "refused",
        "tool = \"smoke\"\n[[do]]\nsay = { text = \"Smoke.\" }\n",
    );
    assert!(load(&tool).unwrap_err().contains("in a skill with a tool"));
}

/// #1684 (a): what a rise can't be is refused with why.
#[test]
fn the_loader_refuses_a_rise_it_cannot_judge() {
    let dir = tempfile::tempdir().unwrap();
    for (trigger, why) in [
        ("rise = { by = 100, within = \"3m\" }", "rise is of a field"),
        (
            "field = \"pm2_5\"\nequals = 5\nrise = { by = 100, within = \"3m\" }",
            "not with equals, below or each",
        ),
        (
            "field = \"pm2_5\"\nbelow = 5\nrise = { by = 100, within = \"3m\" }",
            "not with equals, below or each",
        ),
        (
            "field = \"pm2_5\"\nrise = { by = 0, within = \"3m\" }",
            "above 0",
        ),
        (
            "field = \"pm2_5\"\nrise = { by = -5, within = \"3m\" }",
            "above 0",
        ),
        (
            "field = \"pm2_5\"\nrise = { by = 100, within = \"500ms\" }",
            "1s to 1h",
        ),
        (
            "field = \"pm2_5\"\nrise = { by = 100, within = \"2h\" }",
            "1s to 1h",
        ),
        (
            "field = \"pm2_5\"\nrise = { by = 100, within = \"3 min\" }",
            "not a duration",
        ),
        ("field = \"pm2_5\"\nrise = { by = 100 }", "missing field"),
        (
            "field = \"pm2_5\"\nrise = { by = 100, within = \"3m\", from = 80 }",
            "unknown field",
        ),
    ] {
        let skill = skill_with(
            dir.path(),
            "risen",
            &format!(
                "[trigger]\ntopic = \"air/state\"\n{trigger}\n[ask]\npolicy = \"nudge\"\n[[do]]\nsay = \"Smoke.\"\n"
            ),
        );
        let error = load(&skill).unwrap_err();
        assert!(error.contains(why), "{trigger}: {error}");
    }
    // A rise alone, with no level of its own, holds on any number.
    let skill = skill_with(
        dir.path(),
        "risen",
        "[trigger]\ntopic = \"air/state\"\nfield = \"pm2_5\"\nrise = { by = 50, within = \"1m\" }\n\
         [ask]\npolicy = \"nudge\"\n[[do]]\nsay = \"Smoke.\"\n",
    );
    let readings = air_from(0, 300, |at| Some(if at < 120 { 10.0 } else { 70.0 }));
    assert_eq!(air_runs(&skill, &readings)[0], 120);
}

/// #1684 (b): exact words reach the voice's say service as they are written,
/// as its `text`, byte for byte, with the say's priority and expiry; and
/// what is made with them comes with its `begun`, as for a note.
#[test]
fn exact_words_reach_the_voice_unchanged_at_their_priority() {
    const WORDS: &str = "The air near me just got smoky. If something's burning, please check it. \
                         \u{201c}PM2.5\u{201d} 400 \u{b5}g/m\u{b3} \\ \"quoted\"";
    let skill = format!(
        "[trigger]\ntopic = \"humans/faces/tracked\"\nwhen = \"appears\"\n[ask]\npolicy = \"hello\"\n\
         [[do]]\nlight = {{ colour = \"warm\" }}\nsay = {{ text = '''{WORDS}''', priority = 8, expires = \"2m\" }}\n"
    );
    let seen = the_program_with(
        &skill,
        true,
        &[r#"{"event":"begun","at_us":5}"#, r#"{"outcome":"spoken"}"#],
        3,
        1,
        4,
    );
    let asked: serde_json::Value = serde_json::from_str(seen.asked.as_deref().unwrap()).unwrap();
    assert_eq!(asked["text"].as_str(), Some(WORDS));
    assert_eq!(asked["priority"], 8);
    assert_eq!(asked["expires_in_ms"], 120_000);
    assert_eq!(asked["skill"], "hello");
    assert!(asked.get("note").is_none(), "{asked}");
    let done = &seen.events[3].why;
    assert!(done.contains("light warm: done"), "{done}");
    assert!(done.contains("say: spoken"), "{done}");
    assert!(done.contains("light given back to on blue at 3"), "{done}");
}

/// #1669, Jarvis on #1920: a dotted `rise` field and a dotted `each` list
/// and `key` read inside objects, as a trigger's field does; an empty step
/// is refused at load.
#[test]
fn a_rise_and_an_each_read_a_dotted_path_and_an_empty_step_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let rising = skill_with(
        dir.path(),
        "rising",
        r#"
[trigger]
topic = "air/state"
field = "levels.pm2_5"
rise = { by = 100, within = "1m" }
[ask]
policy = "nudge"
[[do]]
body = { gesture = "nod" }
"#,
    );
    let quiet = r#"{"levels":{"pm2_5":80}}"#;
    let music = r#"{"levels":{"pm2_5":400}}"#;
    let runs: Vec<u64> = runs_at(
        &rising,
        &[
            (0, "air/state", quiet),
            (5, "air/state", quiet),
            (10, "air/state", music),
        ],
    )
    .into_iter()
    .map(|(at, _)| at)
    .collect();
    assert_eq!(runs, [10], "80 to 400 inside levels is a rise");
    let items = skill_with(
        dir.path(),
        "items",
        r#"
[trigger]
topic = "tasks/status"
each = "status.tasks"
key = "meta.task"
field = "state"
equals = "idle"
when = "appears"
[ask]
policy = "news"
[[do]]
body = { gesture = "nod" }
"#,
    );
    let list = |state: &str| {
        format!(r#"{{"status":{{"tasks":[{{"meta":{{"task":1}},"state":"{state}"}}]}}}}"#)
    };
    let (working, idle) = (list("working"), list("idle"));
    let runs: Vec<u64> = runs_at(
        &items,
        &[(0, "tasks/status", &working), (10, "tasks/status", &idle)],
    )
    .into_iter()
    .map(|(at, _)| at)
    .collect();
    assert_eq!(runs, [10], "task 1 under status.tasks turned idle");
    // With the Policy asked, its answer finds the item under the same path.
    let mut engine = Engine::new(vec![load(&items).unwrap()], false).asking_the_policy();
    assert!(
        engine
            .observe("tasks/status", 1, working.as_bytes(), None)
            .is_empty()
    );
    let asked = engine.observe("tasks/status", 2, idle.as_bytes(), None);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        engine.still(&asked[0], 3),
        Some(vec![Call::Gesture("nod".into())]),
        "still idle under status.tasks"
    );
    for (number, field) in ["scores.", ".music", "scores..music"]
        .into_iter()
        .enumerate()
    {
        let error = load(&skill_with(
            dir.path(),
            &format!("typo{number}"),
            &format!(
                "[trigger]\ntopic = \"air/state\"\nfield = \"{field}\"\nabove = 0.5\n[ask]\npolicy = \"nudge\"\n[[do]]\nbody = {{ gesture = \"nod\" }}\n"
            ),
        ))
        .unwrap_err();
        assert!(error.contains("none of them empty"), "{field}: {error}");
    }
}
