//! A voice make's guide check (Heron's P2 on #6768): lamp-tasks starts the
//! make's run with a cleared environment, so the guide's `lamp-skills pack`
//! sees only the switches the voice's line carries
//! (`lamp_tasks::skill_make::CHECK_SWITCHES`). Run here as the real binary
//! with exactly the run's environment, it packs a draft exactly when the
//! voice's own check, run with the second route's worker's switches, does.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const WRITE: &str = "version = \"1.0.0\"\n[trigger]\ntopic = \"room/presence\"\n\
    field = \"present\"\nequals = true\n[ask]\npolicy = \"nudge\"\n[permissions]\n\
    topics = [\"room/presence\"]\nservices = { gmail = \"write\" }\n[[do]]\n\
    write = { service = \"gmail\", action = \"send_mail\", to = [\"boss@work.test\"], \
    subject = \"In\", text = \"I'm at my desk.\" }\n";

const MAIL_TASK: &str = "version = \"1.0.0\"\ngrants = [\"gmail-read\", \"gmail-write\"]\n\
    tool = \"morning_mail\"\n[permissions]\ntopics = [\"clock/local\"]\n\
    [[do]]\ntask = { task = \"Sum up this morning's mail and send it to me\", run = \"scripts/main.py\" }\n";

const PHOTO_TASK: &str = "version = \"1.0.0\"\ngrants = [\"camera\"]\n\
    tool = \"photo_booth\"\n[permissions]\ntopics = [\"clock/local\"]\n\
    [[do]]\ntask = { task = \"Take a photo and send it to my phone\", run = \"scripts/main.py\" }\n";

/// A draft `name` in `dir`, as the guide has Hermes write one, with a
/// script of its own when `script`.
fn draft(dir: &Path, name: &str, toml: &str, script: bool) -> PathBuf {
    let folder = dir.join("draft").join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Does what was asked.\n---\n"),
    )
    .unwrap();
    fs::write(folder.join("skill.toml"), toml).unwrap();
    if script {
        fs::create_dir_all(folder.join("scripts")).unwrap();
        fs::write(folder.join("scripts/main.py"), "print('done')\n").unwrap();
    }
    folder
}

/// Whether the real `lamp-skills pack` packs `folder` with only `env`.
fn packs(folder: &Path, out: &Path, env: &[(&str, String)]) -> bool {
    Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
        .arg("pack")
        .arg("--out")
        .arg(out)
        .arg(folder)
        .env_clear()
        .envs(env.iter().map(|(name, value)| (name, value)))
        .output()
        .unwrap()
        .status
        .success()
}

/// The voice's own check's switches: the worker is given each as an exact
/// `on`, or not at all (`process::agent_services_worker_environment`,
/// `skill_writes_worker_environment`).
fn worker(on: &[&'static str]) -> Vec<(&'static str, String)> {
    on.iter().map(|name| (*name, "on".to_owned())).collect()
}

/// The make run's environment, as lamp-tasks starts it for the voice's
/// guided line with the worker's switches `on`.
fn make_run(folder: &Path, on: &[&'static str]) -> Vec<(&'static str, String)> {
    let switches = lamp_voice::make_skill_agent::check_switches_with(|name| {
        on.contains(&name).then(|| "on".to_owned())
    });
    let line = serde_json::json!({
        "op": "skill_make", "rules": "R", "within_ms": 90_000, "guide": true,
        "switches": switches,
    });
    let make = lamp_tasks::skill_make::MakeLine::parse(line.to_string().as_bytes()).unwrap();
    lamp_tasks::skill_make::make_run_with(
        folder,
        make.within(),
        make.guide,
        &make.switches.unwrap(),
    )
    .env
}

/// lamp-tasks' list is the switches lamp-skills reads as it packs.
#[test]
fn the_makes_switches_are_the_ones_pack_reads() {
    assert_eq!(
        lamp_tasks::skill_make::CHECK_SWITCHES,
        [
            lamp_skills::task::AGENT_SERVICES_ENVIRONMENT,
            lamp_skills::service_write::ENVIRONMENT,
        ]
    );
}

/// A `write` step, and a `do.task` with a script and a linked service's
/// grants, pack inside the make exactly when the voice's check packs them:
/// the switch on, both pack; off, both refuse. Before the line carried
/// them (`make_run`'s environment alone), the make refused both even with
/// the switch on. A `do.task` with a script and a lamp grant packs either
/// way: `LAMP_SKILL_TASKS` and `LAMP_MADE_SCRIPTS` are read as a skill
/// runs or goes on, never at `pack`.
#[test]
fn the_guides_check_packs_a_draft_exactly_when_the_voices_does() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let make = dir.path().join("make-1");
    fs::create_dir_all(&make).unwrap();
    let writes = lamp_skills::service_write::ENVIRONMENT;
    let services = lamp_skills::task::AGENT_SERVICES_ENVIRONMENT;
    for (folder, switch) in [
        (draft(dir.path(), "arrive-mail", WRITE, false), writes),
        (draft(dir.path(), "morning-mail", MAIL_TASK, true), services),
    ] {
        for on in [true, false] {
            let given: &[&str] = if on { &[switch] } else { &[] };
            let voice = packs(&folder, &out, &worker(given));
            let guide = packs(&folder, &out, &make_run(&make, given));
            assert_eq!(voice, on, "{} with {switch} {on}", folder.display());
            assert_eq!(guide, voice, "{} with {switch} {on}", folder.display());
        }
        // The make as it ran before: the switch's line never reached it.
        let before = lamp_tasks::skill_make::make_run(&make, Duration::from_secs(90)).env;
        assert!(!packs(&folder, &out, &before), "{}", folder.display());
    }
    let photo = draft(dir.path(), "photo-booth", PHOTO_TASK, true);
    for given in [&[][..], &[writes, services][..]] {
        assert!(packs(&photo, &out, &worker(given)));
        assert!(packs(&photo, &out, &make_run(&make, given)));
    }
}
