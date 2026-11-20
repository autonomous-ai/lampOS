use super::*;
use crate::{Step, load_with, replay_skill, run_tests, task};
use std::path::PathBuf;
use std::time::Instant;

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

/// The switch on for this thread while it lives, then as it was.
struct On;
impl On {
    fn new(on: bool) -> Self {
        set_on_here(Some(on));
        Self
    }
}
impl Drop for On {
    fn drop(&mut self) {
        set_on_here(None);
    }
}

fn table_of(text: &str) -> toml::Table {
    toml::from_str(text).unwrap()
}

/// `table` without what the variant gives: `grants`, `do` and `present.do`.
fn without_the_line(mut table: toml::Table) -> toml::Table {
    table.remove("grants");
    table.remove("do");
    if let Some(toml::Value::Table(present)) = table.get_mut("present") {
        present.remove("do");
    }
    table
}

/// A copy of `name`'s folder with only its `skill.toml` and `SKILL.md`, as
/// it was before the pilot.
fn core_only(name: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join(name);
    fs::create_dir(&folder).unwrap();
    for file in ["skill.toml", "SKILL.md"] {
        fs::copy(skills_root().join(name).join(file), folder.join(file)).unwrap();
    }
    root
}

/// The tasks of `pilot`, as `(on, task)`.
fn tasks_of(pilot: &Pilot) -> Vec<(Option<String>, String)> {
    pilot
        .tasks
        .iter()
        .map(|task| (task.on.map(str::to_owned), task.task.to_owned()))
        .collect()
}

/// The tasks a loaded skill hands over, as `(on, task)`, each running its
/// brief.
fn tasks_loaded(skill: &crate::Skill) -> Vec<(Option<String>, String)> {
    skill
        .steps
        .iter()
        .filter_map(|(only, step)| match step {
            Step::Call(Call::Task(task)) => {
                assert_eq!(task.run, RUN);
                let on = match only {
                    Some(crate::Only::Trigger(on)) => Some(on.clone()),
                    None => None,
                    Some(other) => panic!("{other:?}"),
                };
                Some((on, task.task.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Every pilot's variant is in the repository, its script the brief byte
/// for byte; the groups are the first builder's (the pilot pair, then the
/// nudges), and a skill left out is no pilot.
#[test]
fn the_pilots_by_group_their_briefs_byte_for_byte() {
    let names = |group: Group| -> Vec<&str> {
        PILOTS
            .iter()
            .filter(|pilot| pilot.group == group)
            .map(|pilot| pilot.name)
            .collect()
    };
    assert_eq!(names(Group::Pilot), ["sip-of-water", "stretch-break"]);
    assert_eq!(
        names(Group::Nudges),
        [
            "stand-up",
            "rest-your-eyes",
            "sit-back",
            "wind-down",
            "comfort-break",
            "meal-check",
            "quiet-focus",
            "stuffy-air"
        ]
    );
    // Left out: a line that names who came, the person's own words, a
    // fixed line, a read, a conversation's tool.
    for left in [
        "welcome-back",
        "reminders",
        "head-pat",
        "smoky-air",
        "dusty-air",
        "morning-agenda",
        "dance",
        "light-show",
    ] {
        assert!(pilot(left).is_none(), "{left}");
    }
    for pilot in &PILOTS {
        // No skill folder of the repository carries a script (the SDK's
        // check, #7862): install and the bundle write the brief.
        let folder = skills_root().join(pilot.name);
        assert!(!folder.join(RUN).exists(), "{}", pilot.name);
        assert!(folder.join(OVERLAY).is_file(), "{}", pilot.name);
        let written = script(pilot);
        // The lamp's own copy install and the bundle write, this crate's.
        let kept = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/core_agent/briefs")
            .join(format!("{}.py", pilot.name));
        assert_eq!(
            fs::read_to_string(&kept).unwrap(),
            written,
            "{}",
            pilot.name
        );
        assert!(written.starts_with("print(\"") && written.ends_with("\")\n"));
        let inside = &written["print(\"".len()..written.len() - "\")\n".len()];
        assert!(
            !inside.contains(['"', '\\', '`']),
            "one plain string: {inside}"
        );
        assert!(written.contains(pilot.says), "{written}");
        assert!(written.contains("answer -."), "{written}");
        for task in pilot.tasks {
            assert!(task.task.len() <= task::MOST_TASK_BYTES, "{}", pilot.name);
            assert!(!task.task.contains(['"', '\\', '`']), "{}", pilot.name);
        }
    }
    assert_eq!(FALLBACK_WITHIN, Duration::from_secs(10));
}

/// With the switch on, the trigger, the guards and the timing are the core
/// skill's own: the parsed tomls are the same but for `grants`, `[[do]]`
/// and `[[present.do]]`, every step but the task is the core one word for
/// word, and the loaded skill is the core skill but for its task.
#[test]
fn with_the_switch_on_the_trigger_guards_and_timing_are_the_core_ones() {
    for pilot in &PILOTS {
        let _on = On::new(true);
        let (_root, folder) = whole(pilot.name);
        let core = table_of(&fs::read_to_string(folder.join("skill.toml")).unwrap());
        let variant = overlaid(pilot.name, &folder, &core)
            .unwrap()
            .unwrap_or_else(|| panic!("{}: a variant", pilot.name));
        assert_eq!(
            without_the_line(variant.clone()),
            without_the_line(core.clone()),
            "{}",
            pilot.name
        );
        for key in [
            "trigger",
            "while",
            "most_a_day",
            "active",
            "quiet",
            "budget",
            "rest",
        ] {
            assert_eq!(variant.get(key), core.get(key), "{}: {key}", pilot.name);
        }
        assert_eq!(variant["grants"], toml::Value::Array(Vec::new()));
        for present in [false, true] {
            let _off = On::new(false);
            let own = load_with(&folder, present).unwrap();
            drop(_off);
            let _on = On::new(true);
            let with = load_with(&folder, present).unwrap();
            assert!(with.via_agent && !own.via_agent, "{}", pilot.name);
            assert_eq!(own.task_step(), None);
            assert_eq!(tasks_loaded(&with), tasks_of(pilot), "{}", pilot.name);
            assert!(with.grants.is_empty());
            let mut same = with.clone();
            same.steps
                .retain(|(_, step)| !matches!(step, Step::Call(Call::Task(_))));
            same.via_agent = false;
            assert_eq!(same, own, "{} present {present}", pilot.name);
        }
    }
}

/// The variant stands in only when it is exactly the pilot's: another
/// script, a grant, another task, a changed fallback, a trigger of its own,
/// or a skill not a pilot leaves the skill as it is.
#[test]
fn a_variant_not_exactly_the_pilots_leaves_the_core_skill_as_it_is() {
    let _on = On::new(true);
    let copy = |name: &str| {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join(name);
        fs::create_dir_all(folder.join("scripts")).unwrap();
        for file in ["skill.toml", "SKILL.md", OVERLAY] {
            fs::copy(
                skills_root().join("sip-of-water").join(file),
                folder.join(file),
            )
            .unwrap();
        }
        fs::write(folder.join(RUN), script(pilot("sip-of-water").unwrap())).unwrap();
        (root, folder)
    };
    let (_root, folder) = copy("sip-of-water");
    let core = table_of(&fs::read_to_string(folder.join("skill.toml")).unwrap());
    assert!(overlaid("sip-of-water", &folder, &core).unwrap().is_some());
    let overlay = fs::read_to_string(folder.join(OVERLAY)).unwrap();
    for (what, file, from, to) in [
        (
            "a script of its own",
            RUN,
            "answer -.",
            "answer -. import os",
        ),
        ("a grant", OVERLAY, "grants = []", "grants = [\"speak\"]"),
        (
            "another task",
            OVERLAY,
            "suggesting a sip",
            "suggesting a coffee",
        ),
        (
            "another fallback",
            OVERLAY,
            "for about half an hour",
            "for an hour",
        ),
        (
            "a trigger",
            OVERLAY,
            "grants = []",
            "grants = []\nmost_a_day = 9",
        ),
    ] {
        let _on = On::new(true);
        let (_root, folder) = copy("sip-of-water");
        let text = fs::read_to_string(folder.join(file)).unwrap();
        assert!(text.contains(from), "{what}");
        fs::write(folder.join(file), text.replacen(from, to, 1)).unwrap();
        assert!(overlaid("sip-of-water", &folder, &core).is_err(), "{what}");
        let skill = load_with(&folder, false).unwrap();
        assert!(!skill.via_agent && skill.task_step().is_none(), "{what}");
        let _off = On::new(false);
        assert_eq!(skill, load_with(&folder, false).unwrap(), "{what}");
    }
    // A skill not a pilot never reads one, the same file beside it.
    let _on = On::new(true);
    let (_root, other) = copy("head-pat");
    assert_eq!(overlaid("head-pat", &other, &core), Ok(None));
    assert_eq!(overlay.matches("task = {").count(), 2);
}

/// The switch's value turns on the groups it names, `on` every one, and
/// nothing else: the pilot pair and the nudges each go on by their own.
#[test]
fn the_switch_turns_on_the_groups_it_names() {
    for (value, pilot, nudges) in [
        (None, false, false),
        (Some("on"), true, true),
        (Some("pilot"), true, false),
        (Some("nudges"), false, true),
        (Some("pilot, nudges"), true, true),
        (Some("off"), false, false),
        (Some("ON"), false, false),
        (Some("pilots"), false, false),
        (Some(""), false, false),
    ] {
        assert_eq!(turns_on(value, Group::Pilot), pilot, "{value:?}");
        assert_eq!(turns_on(value, Group::Nudges), nudges, "{value:?}");
    }
}

/// Each fire hands the lamp's agent its task: in every recorded case, each
/// say of the variant comes with its `do.task`, the skill's own intent
/// running its brief, handed to lamp-tasks with no grant.
#[test]
fn each_fire_hands_the_agent_its_task() {
    let _on = On::new(true);
    for pilot in &PILOTS {
        let (_root, folder) = whole(pilot.name);
        let mut fires = 0;
        for entry in fs::read_dir(folder.join("tests")).unwrap().flatten() {
            let case = entry.path();
            if !case.is_dir() || crate::read_recordings(&case).unwrap().is_empty() {
                continue;
            }
            let expect: toml::Table =
                toml::from_str(&fs::read_to_string(case.join("expect.toml")).unwrap()).unwrap();
            let present = expect.get("present").and_then(toml::Value::as_bool) == Some(true);
            let clock = expect.get("clock").and_then(toml::Value::as_str);
            let skill = load_with(&folder, present).unwrap();
            assert!(skill.via_agent);
            let made = replay_skill(skill, &case, &[], clock, 1).unwrap();
            let says: Vec<u64> = made
                .iter()
                .filter(|made| is_line(&made.call))
                .map(|made| made.at_us)
                .collect();
            let tasks: Vec<u64> = made
                .iter()
                .filter_map(|made| match &made.call {
                    Call::Task(step) => {
                        assert_eq!(step.run, RUN);
                        assert!(
                            pilot.tasks.iter().any(|task| task.task == step.task),
                            "{}",
                            pilot.name
                        );
                        Some(made.at_us)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(says, tasks, "{}", case.display());
            fires += tasks.len();
        }
        assert!(fires > 0, "{}", pilot.name);
        // What lamp-tasks receives of it.
        let skill = load_with(&folder, false).unwrap();
        let step = skill.task_step().unwrap();
        let line = task::HandOver {
            skill: pilot.name,
            step,
            folder: &folder,
            sha256: "0",
            grants: &skill.grants,
        }
        .line();
        let line: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(line["op"], "skill_task");
        assert_eq!(line["task"], pilot.tasks[0].task);
        assert_eq!(line["run"], RUN);
        assert_eq!(line["grants"], serde_json::json!([]));
    }
}

/// Each skill's own `tests/` pass against the new form: a case has no
/// agent, so each fire says the skill's own line at once, as the fallback
/// does; and the same cases pass with the switch off.
#[test]
fn each_skills_own_tests_pass_against_the_new_form() {
    for pilot in &PILOTS {
        let (_root, folder) = whole(pilot.name);
        let off = {
            let _off = On::new(false);
            run_tests(&folder).unwrap()
        };
        let _on = On::new(true);
        let on = run_tests(&folder).unwrap_or_else(|why| panic!("{why}"));
        assert_eq!(on, off, "{}", pilot.name);
        assert!(on.0 > 0);
    }
}

/// With the switch off nothing changes: the folder, its variant and script
/// beside it, loads exactly as the core skill did without them, with no
/// task and no grant; the variant is never read (an unreadable one too).
#[test]
fn with_the_switch_off_nothing_changes() {
    let _off = On::new(false);
    for pilot in &PILOTS {
        let before = core_only(pilot.name);
        for present in [false, true] {
            let now = load_with(&skills_root().join(pilot.name), present).unwrap();
            let was = load_with(&before.path().join(pilot.name), present).unwrap();
            assert_eq!(now, was, "{} present {present}", pilot.name);
            assert!(!now.via_agent && now.task_step().is_none() && now.grants.is_empty());
        }
        let core = table_of(
            &fs::read_to_string(skills_root().join(pilot.name).join("skill.toml")).unwrap(),
        );
        let broken = tempfile::tempdir().unwrap();
        fs::write(broken.path().join(OVERLAY), "not toml [").unwrap();
        assert_eq!(overlaid(pilot.name, broken.path(), &core), Ok(None));
    }
}

fn answer(text: &str) -> String {
    serde_json::json!({"ok": true, "id": 7, "answer": text}).to_string()
}

/// The agent's line in time is said in the say's place, as exact words at
/// the say's priority; the look and the light stay, and the fallback keeps
/// every call as it is.
#[test]
fn the_agents_line_in_time_is_said_in_the_says_place() {
    let calls = vec![
        Call::Gaze { face: None },
        Call::Say("They have sat a while. Suggest a sip of water.".into()),
    ];
    let (to, from) = mpsc::channel();
    let heard = heard(task::told(&answer("Time for a sip of water?")));
    assert_eq!(heard, Heard::Line("Time for a sip of water?".into()));
    to.send(heard).unwrap();
    let said = awaited(from, Duration::from_millis(500));
    assert_eq!(said, Said::Agent("Time for a sip of water?".into()));
    assert!(!said.journal().contains("sip"), "never the agent's words");
    let made = with_line(calls.clone(), &said);
    assert_eq!(made[0], calls[0]);
    let Call::Words(words) = &made[1] else {
        panic!("{made:?}");
    };
    assert_eq!(words.words, "Time for a sip of water?");
    assert!(words.exact);
    assert_eq!(words.priority, SAY_PRIORITY);
    let fallback = Said::Fallback(NOT_RUN.into());
    assert_eq!(with_line(calls.clone(), &fallback), calls);
}

/// A slow run: its line not come within 10 s of the fire, the skill's own
/// is said at once, never later; a line after it is dropped.
#[test]
fn a_slow_agent_past_10_s_says_the_skills_own_line_at_once() {
    let (to, from) = mpsc::channel();
    let began = Instant::now();
    let late = std::thread::spawn(move || {
        std::thread::sleep(FALLBACK_WITHIN + Duration::from_millis(400));
        to.send(Heard::Line("Have some water!".into()))
    });
    let said = awaited(from, FALLBACK_WITHIN);
    let waited = began.elapsed();
    assert!(matches!(said, Said::Fallback(_)), "{said:?}");
    assert!(said.journal().contains("within 10 s"), "{}", said.journal());
    assert!(waited >= FALLBACK_WITHIN, "{waited:?}");
    assert!(
        waited < FALLBACK_WITHIN + Duration::from_millis(300),
        "{waited:?}"
    );
    // The late line finds no say waiting: never said.
    assert!(late.join().unwrap().is_err());
}

/// The agent down, offline, not reached, refused or waiting for the yes:
/// its run ends with no line (its sender gone, or nothing), and the skill's
/// own is said at once, not after 10 s.
#[test]
fn the_agent_down_says_the_skills_own_line_at_once() {
    for refused in [
        r#"{"ok":false,"error":"busy","message":"another skill's run"}"#,
        r#"{"ok":false,"error":"refused","message":"Harness didn't start the run (harness_not_running)"}"#,
        "not json",
    ] {
        assert_eq!(heard(task::told(refused)), Heard::Nothing(NOTHING_TO_SAY));
    }
    let began = Instant::now();
    let (to, from) = mpsc::channel::<Heard>();
    drop(to);
    let said = awaited(from, FALLBACK_WITHIN);
    assert_eq!(said, Said::Fallback(NOT_RUN.into()));
    let (to, from) = mpsc::channel();
    to.send(heard(None)).unwrap();
    assert_eq!(
        awaited(from, FALLBACK_WITHIN),
        Said::Fallback(NOTHING_TO_SAY.into())
    );
    assert!(began.elapsed() < Duration::from_secs(1));
}

/// The agent answering `-`, nothing to say: the skill's own line at once.
#[test]
fn a_dash_says_the_skills_own_line_at_once() {
    for dash in ["-", " - ", "--", "-\n"] {
        let (to, from) = mpsc::channel();
        to.send(heard(task::told(&answer(dash)))).unwrap();
        assert_eq!(
            awaited(from, FALLBACK_WITHIN),
            Said::Fallback(NOTHING_TO_SAY.into()),
            "{dash:?}"
        );
    }
}

/// An empty answer, or none at all: the skill's own line at once; and an
/// answer longer than one short line is never said either.
#[test]
fn an_empty_answer_says_the_skills_own_line_at_once() {
    for empty in [
        answer(""),
        answer("   "),
        answer("**"),
        r#"{"ok":true,"id":7}"#.to_owned(),
    ] {
        let (to, from) = mpsc::channel();
        to.send(heard(task::told(&empty))).unwrap();
        assert_eq!(
            awaited(from, FALLBACK_WITHIN),
            Said::Fallback(NOTHING_TO_SAY.into()),
            "{empty}"
        );
    }
    let long = answer("Drink. Water is good. Hydration matters for your health.");
    assert_eq!(heard(task::told(&long)), Heard::Nothing(TOO_LONG));
    let long = answer(&"water ".repeat(60));
    assert_eq!(heard(task::told(&long)), Heard::Nothing(TOO_LONG));
}

/// A copy of `name`'s whole folder, its variant beside it and its brief
/// written, as install and the bundle place it on a lamp.
fn whole(name: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join(name);
    copy_folder(&skills_root().join(name), &folder);
    if let Some(pilot) = pilot(name) {
        fs::create_dir_all(folder.join("scripts")).unwrap();
        fs::write(folder.join(RUN), script(pilot)).unwrap();
    }
    (root, folder)
}

fn copy_folder(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            copy_folder(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

/// The core variants' tasks are pre-approved (the first builder, 19 Nov
/// 08:57: "They're ours and have no grants"): the first fire, and the ones
/// after it, hand the task over with no yes asked aloud and none asked of
/// lamp-control; a folder that is not exactly a variant is never
/// pre-approved, whatever the switch.
#[test]
fn the_first_fire_asks_nothing() {
    let _on = On::new(true);
    task::set_on_here(Some(true));
    for pilot in &PILOTS {
        let (root, folder) = whole(pilot.name);
        assert!(pre_approved(&folder), "{}", pilot.name);
        let state = root.path().join("state");
        fs::create_dir_all(&state).unwrap();
        // No lamp-control here: asked, it could only say it can't say.
        let control = root.path().join("no-control.sock");
        let tasks = root.path().join("tasks.sock");
        let skill = load_with(&folder, false).unwrap();
        let step = skill.task_step().unwrap();
        for fire in 0..3 {
            let place = task::Place {
                state: Some(&state),
                control: &control,
                socket: Some(&tasks),
            };
            let mut sent = None;
            let said = task::hand(
                pilot.name,
                step,
                &folder,
                &skill.grants,
                place,
                1 + fire * 3_600_000,
                |_, line| {
                    sent = Some(line.to_owned());
                    crate::service_write::agent_answered("{\"ok\":true,\"id\":9}")
                },
                || -> String { panic!("{}: a yes asked aloud", pilot.name) },
            );
            assert!(said.starts_with("do.task: handed to lamp-tasks"), "{said}");
            let line: serde_json::Value = serde_json::from_str(&sent.unwrap()).unwrap();
            assert_eq!(line["grants"], serde_json::json!([]));
        }
        assert!(!state.join(task::ASKS_FILE).exists(), "{}", pilot.name);
    }
    // Pre-approval is the folder's own, never the switch's.
    let (_root, folder) = whole("sip-of-water");
    set_on_here(Some(false));
    assert!(pre_approved(&folder));
    set_on_here(Some(true));
    // Not exactly a variant: the core skill alone, a changed brief, a skill
    // that is no pilot, or a pilot's variant under another name.
    let root = core_only("sip-of-water");
    assert!(!pre_approved(&root.path().join("sip-of-water")));
    let (_root, folder) = whole("stretch-break");
    fs::write(folder.join(RUN), "print(\"Say what you like.\")\n").unwrap();
    assert!(!pre_approved(&folder));
    let (_root, folder) = whole("head-pat");
    assert!(!pre_approved(&folder));
    let (root, folder) = whole("sip-of-water");
    let renamed = root.path().join("photo-booth");
    fs::rename(&folder, &renamed).unwrap();
    assert!(!pre_approved(&renamed));
    task::set_on_here(None);
}
