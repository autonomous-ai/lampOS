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

/// #7245: the minimal skill.toml a gateway make's words show inline
/// (`lamp_tasks::skill_make::FORM_TOML`) packs with the real `lamp-skills
/// pack`, as a skill made on this lamp (`made: here`) with
/// `LAMP_MADE_SKILL_ASKS` on; its `asks` moved under `[permissions]`, as
/// the inline rule forbids, is refused.
#[test]
fn the_gateway_makes_inline_skill_toml_packs() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let toml = lamp_tasks::skill_make::FORM_TOML;
    let asks = lamp_voice::made_skill::ASKS_ENVIRONMENT;
    let made = |name: &str, toml: &str| {
        let folder = draft(dir.path(), name, toml, false);
        fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Glows warm.\nmade: here\n---\n"),
        )
        .unwrap();
        folder
    };
    let on = [(asks, "on".to_owned())];
    assert!(packs(&made("desk-glow", toml), &out, &on));
    let (line, rest) = toml.split_once('\n').unwrap();
    let under = rest.replace("light = true\n", &format!("light = true\n{line}\n"));
    assert!(!under.starts_with("asks"));
    assert!(!packs(&made("desk-glow-under", &under), &out, &on));
}

/// #7245: each example skill a gateway make's folder holds
/// (`lamp_tasks::skill_make::MAKE_EXAMPLES`) packs with the real
/// `lamp-skills pack`, as a skill made on this lamp with
/// `LAMP_MADE_SKILL_ASKS` on: an example can never be one the check
/// refuses. It names each of its eleven skills once, in order.
#[test]
fn every_gateway_make_example_packs() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let examples = dir.path().join("examples");
    let mut names: Vec<&str> = Vec::new();
    for (path, text) in lamp_tasks::skill_make::MAKE_EXAMPLES {
        let file = examples.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, text).unwrap();
        let name = path.split('/').next().unwrap();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    assert_eq!(
        names,
        [
            "desk-light",
            "stretch-break",
            "coin-flip",
            "water-while-here",
            "break-timer",
            "breathing-coach",
            "exchange-rate",
            "water-breaks",
            "agenda-count",
            "sun-times",
            "morning-headline",
        ]
    );
    // With the internet switch too, as a gateway make whose agent may
    // write net code is checked (#7742): the exchange-rate, sun-times and
    // morning-headline examples are granted `net`; and a linked service's read with the
    // voice's switch for one on (agenda-count).
    let asks = (lamp_voice::made_skill::ASKS_ENVIRONMENT, "on".to_owned());
    let on = [
        asks.clone(),
        (lamp_tasks::skill_task::NET_SWITCH, "on".to_owned()),
        (
            lamp_voice::made_skill::AGENT_SERVICES_ENVIRONMENT,
            "on".to_owned(),
        ),
    ];
    for name in names {
        assert!(packs(&examples.join(name), &out, &on), "{name}");
    }
    // Off, a skill its own words start is refused, as before, and `net`
    // is no grant where its switch is off.
    assert!(!packs(&examples.join("desk-light"), &out, &[]));
    assert!(!packs(&examples.join("sun-times"), &out, &[asks]));
}

/// #7245: a made skill's light show, its own words alone starting it,
/// packs exactly when lamp-skills' own show check takes it: 15 frames of
/// 3 s (45 s) do; 31 frames, a 1 s frame, a level of 6, or a trigger
/// beside its words, don't.
#[test]
fn a_made_skills_light_show_packs_only_as_the_show_check_takes_it() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let on = [(lamp_voice::made_skill::ASKS_ENVIRONMENT, "on".to_owned())];
    let toml = "asks = [\"traffic light\"]\nkind = \"installable\"\nversion = \"1.0.0\"\n\n\
                [permissions]\nlight = true\n\n[[do]]\nshow = \"traffic-light\"\n";
    let show = |frames: usize, lasting: &str, level: u8| {
        (0..frames)
            .map(|at| {
                let colour = ["green", "yellow", "red"][at % 3];
                format!(
                    "[[light]]\ncolour = \"{colour}\"\nlevel = {level}\nfor = \"{lasting}\"\n\n"
                )
            })
            .collect::<String>()
    };
    let made = |name: &str, toml: &str, frames: &str| {
        let folder = draft(dir.path(), name, toml, false);
        fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: A traffic light show.\nmade: here\n---\n"),
        )
        .unwrap();
        fs::create_dir_all(folder.join("shows")).unwrap();
        fs::write(folder.join("shows/traffic-light.toml"), frames).unwrap();
        folder
    };
    assert!(packs(&made("traffic", toml, &show(15, "3s", 4)), &out, &on));
    assert!(packs(&made("flash", toml, &show(5, "2s", 5)), &out, &on));
    for (name, frames) in [
        ("many", show(31, "2s", 4)),
        ("fast", show(3, "1s", 4)),
        ("bright", show(3, "3s", 6)),
    ] {
        assert!(!packs(&made(name, toml, &frames), &out, &on), "{name}");
    }
    let triggered = toml.replace(
        "[[do]]",
        "[trigger]\ntopic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\n[[do]]",
    );
    let triggered = triggered.replace(
        "light = true\n",
        "light = true\ntopics = [\"room/presence\"]\n",
    );
    assert!(!packs(
        &made("triggered", &triggered, &show(3, "3s", 4)),
        &out,
        &on
    ));
}
