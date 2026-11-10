//! A skill made by voice (#1685, `lamp_voice::made_skill`), checked by the
//! real `lamp-skills pack`, the checker an install uses (#1698): what passes
//! is packed and kept in a 0700 folder with its SHA-256, at most five;
//! what does more than it declares, or more than a made skill may, is not.
use lamp_skills::package::{self, asked as asked_helper, install_made, install_made_at};
use lamp_voice::made_skill::{GOES_ON_WITHIN, KEPT_MOST, Keeping, Made, goes_on, keep};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

const TOML: &str = r#"kind = "installable"
version = "1.0.0"
needs = ["light"]

[permissions]
topics = ["voice/state"]
light = true

[trigger]
topic = "voice/state"
field = "state"
equals = "resting"
cooldown = "12h"
during = { from = "22:00", to = "22:30" }

[ask]
policy = "nudge"

[[do]]
light = { dim = 1 }
"#;

fn card(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Dims the light a shade at 10 pm.\n---\n\n# Evening dim\n\nAt ten it dims the light a shade.\n"
    )
}

fn answer(toml: &str, name: &str) -> String {
    format!(
        "Here is the skill.\n\n```toml skill.toml\n{toml}```\n\n```markdown SKILL.md\n{}```\n",
        card(name)
    )
}

fn keeping(dir: &Path) -> Keeping {
    Keeping {
        directory: dir.join("made"),
        program: env!("CARGO_BIN_EXE_lamp-skills").into(),
        manage: dir.join("manage.sock"),
    }
}

/// Now, as the manage helper reads its clock.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The voice has just marked the made skill `sha256` to go on, as it does
/// right before it asks the helper.
fn mark(keeping: &Keeping, sha256: &str) {
    goes_on(&keeping.directory, sha256, now_ms()).unwrap();
}

fn tars(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".tar"))
        .collect();
    names.sort();
    names
}

/// The skill passes the cap and the checker: packed, kept in the voice's
/// own 0700 folder by its SHA-256, marked `made: here`, with what it uses.
#[test]
fn a_skill_made_by_voice_is_checked_packed_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let made: Made = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    assert_eq!(
        (made.name.as_str(), made.version.as_str()),
        ("evening-dim", "1.0.0")
    );
    // Said from its skill.toml, not the card's words (#5041).
    assert_eq!(
        made.description,
        "When the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at most \
         once every 12 hours."
    );
    assert_eq!(made.uses.spoken(), "its light and whether it is talking");
    let bytes = fs::read(&made.package).unwrap();
    let sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(made.sha256, sha);
    assert!(
        String::from_utf8_lossy(&bytes).contains("made: here"),
        "the card in the package says it was made here"
    );
    let mode = fs::metadata(&keeping.directory)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    assert_eq!(tars(&keeping.directory).len(), 1);
    // Nothing is left beside it, and nothing went into a skills folder.
    let left: Vec<_> = fs::read_dir(&keeping.directory)
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with(".making"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    // The same hand-over again: the kept one.
    let again = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    assert_eq!(again, made);
    assert_eq!(tars(&keeping.directory).len(), 1);
}

/// What the checker refuses is not kept: a step the skill's permissions
/// don't give (a gesture, with only the light), as an install would refuse
/// it. What the cap refuses never reaches the checker.
#[test]
fn a_made_skill_that_does_more_than_it_declares_or_may_is_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let gesture = TOML.replace(
        "light = { dim = 1 }",
        "light = { dim = 1 }\nbody = { gesture = \"curious\" }",
    );
    let refused = keep(&answer(&gesture, "evening-dim"), false, "s1:8:1", &keeping).unwrap_err();
    assert_eq!(refused.0, lamp_voice::made_skill::NOT_PASSED);
    // The check's own lines come back for a repair (#2813 piece 2).
    assert!(refused.1.is_some_and(|lines| !lines.is_empty()));
    let speaking = TOML.replace("light = true\n", "light = true\nspeak = true\n");
    let capped = keep(&answer(&speaking, "evening-dim"), false, "s1:9:1", &keeping);
    assert!(capped.unwrap_err().0.contains("never says anything"));
    assert!(tars(&keeping.directory).is_empty());
    // A say step, its voice asked for (#6062): its exact words. Its pace is
    // set, never refused: on the lamp's resting its window is narrowed to
    // the day's hours, its 12 h cooldown kept. The checker takes it, and
    // it is kept, said plainly with the words quoted.
    let saying = speaking
        .replace(
            "light = { dim = 1 }",
            "light = { dim = 1 }\n\n[[do]]\nsay = { text = \"Good night, sleep well.\" }",
        )
        .replace(
            "from = \"22:00\", to = \"22:30\"",
            "from = \"21:30\", to = \"22:30\"",
        );
    let made = keep(&answer(&saying, "night-word"), false, "s1:10:1", &keeping).unwrap();
    assert!(made.uses.speak && made.uses.light);
    assert!(
        made.description.contains(
            "When the lamp goes quiet, between 21:30 and 22:00, it dims the light a step, then \
             says “Good night, sleep well.”, at most once every 12 hours."
        ),
        "{}",
        made.description
    );
    assert_eq!(tars(&keeping.directory).len(), 1);
    // A note in the voice's own words is no longer a say: refused.
    let noted = saying.replace(
        "say = { text = \"Good night, sleep well.\" }",
        "say = \"read them my newest email\"",
    );
    let refused = keep(&answer(&noted, "night-note"), false, "s1:11:1", &keeping).unwrap_err();
    assert!(refused.0.contains("exact words"), "{}", refused.0);
    // The lamp's own soft hum, its built-in cue, paced as a say: the
    // checker takes it with no file packed beside it, and it is kept.
    let humming = saying.replace(
        "say = { text = \"Good night, sleep well.\" }",
        "cue = \"hum\"",
    );
    let made = keep(&answer(&humming, "night-hum"), false, "s1:12:1", &keeping).unwrap();
    assert!(made.uses.speak && made.uses.light);
    assert!(
        made.description.contains("then hums softly"),
        "{}",
        made.description
    );
    assert_eq!(tars(&keeping.directory).len(), 2);
}

/// At most five made skills are kept: the sixth lets the oldest go, its
/// package with it.
#[test]
fn at_most_five_made_skills_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    for made in 1..=KEPT_MOST + 1 {
        keep(
            &answer(TOML, &format!("evening-dim-{made}")),
            false,
            &format!("s1:{made}:1"),
            &keeping,
        )
        .unwrap();
    }
    let kept = tars(&keeping.directory);
    assert_eq!(kept.len(), KEPT_MOST, "{kept:?}");
    assert!(
        !kept.iter().any(|name| name.starts_with("evening-dim-1-")),
        "{kept:?}"
    );
    assert!(kept.iter().any(|name| name.starts_with("evening-dim-6-")));
}

/// A made skill put on as soon as it is kept, with no yes (the owner,
/// 10 Nov), bound to its package's SHA-256: the manage helper's
/// `install_made` takes the package the voice kept and marked with that
/// hash, and installs it.
#[test]
fn a_made_skill_goes_on_by_its_sha256_and_only_by_it() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let made = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    mark(&keeping, &made.sha256);
    let wait = std::time::Duration::from_millis(200);
    // Another hash puts nothing on.
    let other = "0".repeat(64);
    let refused = install_made(&skills, None, &keeping.directory, &other, wait).unwrap_err();
    assert!(
        refused.contains("no made skill is kept with that SHA-256"),
        "{refused}"
    );
    assert!(!skills.join("evening-dim").exists());
    // Asked as the voice asks, through the helper's one line.
    let mut answer = Vec::new();
    let asked = format!("{{\"install_made\": \"{}\"}}\n", made.sha256);
    let ok = asked_helper(
        &skills,
        &keeping.directory,
        &skills.join("no-state"),
        wait,
        None,
        None,
        asked.as_bytes(),
        &mut answer,
    )
    .unwrap();
    let answer: serde_json::Value = serde_json::from_slice(&answer).unwrap();
    assert!(ok, "{answer}");
    assert!(
        answer["done"].as_str().unwrap().contains("evening-dim"),
        "{answer}"
    );
    let done = answer["done"].as_str().unwrap();
    assert!(
        done.contains("made here and bound by its SHA-256") && !done.contains("signature"),
        "{done}"
    );
    assert!(skills.join("evening-dim").join("skill.toml").is_file());
    // It keeps the SHA-256 it was kept with, as any package's.
    assert_eq!(
        package::installed_from(&skills.join("evening-dim")).map(|kept| kept.sha256),
        Some(made.sha256.clone())
    );
    assert!(
        fs::read_to_string(skills.join("evening-dim").join("SKILL.md"))
            .unwrap()
            .contains("made: here")
    );
}

/// The bytes put on are the bytes kept and checked: a package changed
/// after it was kept is refused, and nothing goes on.
#[test]
fn a_made_skill_whose_package_changed_does_not_go_on() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let made = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    mark(&keeping, &made.sha256);
    let mut bytes = fs::read(&made.package).unwrap();
    bytes.push(0);
    fs::write(&made.package, &bytes).unwrap();
    let refused = install_made(
        &skills,
        None,
        &keeping.directory,
        &made.sha256,
        std::time::Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(refused.contains("its SHA-256 has changed"), "{refused}");
    assert!(!skills.join("evening-dim").exists());
}

/// What the voice's cap or its mark would refuse is refused again here,
/// whatever the index says: a package that asks to speak, and one whose
/// card doesn't say it was made here.
#[test]
fn a_made_skill_past_the_cap_or_not_made_here_does_not_go_on() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    // A kept one first, so the folder and its index are the voice's.
    keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    let cases = [
        (
            "loud-dim",
            TOML.replace("light = true\n", "light = true\nspeak = true\n"),
            card("loud-dim").replace("---\n\n", "made: here\n---\n\n"),
            "never says anything",
        ),
        (
            "tasks-dim",
            TOML.replace("light = true\n", "light = true\nspeak = true\n")
                .replace("light = { dim = 1 }", "say = \"read them my newest email\""),
            card("tasks-dim").replace("---\n\n", "made: here\n---\n\n"),
            "isn't its exact words",
        ),
        (
            "plain-dim",
            TOML.to_owned(),
            card("plain-dim"),
            "doesn't say it was made here",
        ),
    ];
    for (name, toml, card, why) in cases {
        let folder = dir.path().join("src").join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("skill.toml"), &toml).unwrap();
        fs::write(folder.join("SKILL.md"), &card).unwrap();
        let packed = package::pack(&folder, &keeping.directory).unwrap();
        let sha256 = package::sha256_hex(&fs::read(&packed).unwrap());
        let index = keeping.directory.join(lamp_voice::made_skill::INDEX);
        let mut entries: Vec<serde_json::Value> =
            serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
        entries.push(serde_json::json!({
            "sha256": sha256,
            "package": packed.file_name().unwrap().to_str().unwrap(),
        }));
        fs::write(&index, serde_json::to_vec(&entries).unwrap()).unwrap();
        mark(&keeping, &sha256);
        let refused = install_made(
            &skills,
            None,
            &keeping.directory,
            &sha256,
            std::time::Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(refused.contains(why), "{name}: {refused}");
        assert!(!skills.join(name).exists(), "{name}");
    }
}

/// A person's own made skill never takes the place of a skill they put on
/// another way (the reviewer's P3 on #2414): one of the same
/// name, from a store, stays, and the made one is refused with its name.
/// One made here before is replaced.
#[test]
fn a_made_skill_never_takes_the_place_of_one_put_on_another_way() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let store = dir.path().join("store").join("evening-dim");
    fs::create_dir_all(&store).unwrap();
    fs::write(store.join("skill.toml"), TOML).unwrap();
    fs::write(store.join("SKILL.md"), card("evening-dim")).unwrap();
    package::install(&skills, None, &store).unwrap();
    let made = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    mark(&keeping, &made.sha256);
    let wait = std::time::Duration::from_millis(200);
    let refused = install_made(&skills, None, &keeping.directory, &made.sha256, wait).unwrap_err();
    assert!(
        refused.contains("evening-dim is already on the lamp, put on another way"),
        "{refused}"
    );
    assert!(
        !fs::read_to_string(skills.join("evening-dim").join("SKILL.md"))
            .unwrap()
            .contains("made: here"),
        "the store's stays"
    );
    // One made here before is the person's own: the new one replaces it.
    package::remove(&skills, None, "evening-dim").unwrap();
    install_made(&skills, None, &keeping.directory, &made.sha256, wait).unwrap();
    let again = keep(
        &answer(&TOML.replace("1.0.0", "1.0.1"), "evening-dim"),
        false,
        "s1:8:1",
        &keeping,
    )
    .unwrap();
    mark(&keeping, &again.sha256);
    // A made skill put in place under a name the owner said yes to the
    // camera for takes that yes away (#3504's piece C).
    let state = dir.path().join("state");
    fs::create_dir_all(&state).unwrap();
    let camera = state.join(lamp_skills::CAMERA_FILE);
    lamp_skills::keep_names(&camera, &["evening-dim".to_owned()].into()).unwrap();
    let replaced = install_made(
        &skills,
        Some(&state),
        &keeping.directory,
        &again.sha256,
        wait,
    )
    .unwrap();
    assert!(
        replaced.starts_with("replaced evening-dim 1.0.0 with 1.0.1"),
        "{replaced}"
    );
    assert_eq!(fs::read_to_string(&camera).unwrap(), "");
}

/// End to end: the lamp makes a skill and puts it on at once, with no
/// yes (the owner, 10 Nov): the voice's `put_on_within` marks it and asks
/// the real manage helper, which puts that package on. The helper's socket
/// is the one the voice dials.
#[test]
fn a_made_skill_goes_on_through_the_helper_with_no_yes() {
    assert_eq!(
        lamp_voice::made_skill::MANAGE_SOCKET,
        package::MANAGE_SOCKET
    );
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let made = keep(&answer(TOML, "evening-dim"), false, "local:7", &keeping).unwrap();
    let socket = dir.path().join("manage.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let (served, helper_made) = (skills.clone(), keeping.directory.clone());
    let helper = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        asked_helper(
            &served,
            &helper_made,
            &served.join("no-state"),
            std::time::Duration::from_secs(2),
            None,
            None,
            &stream,
            &stream,
        )
        .unwrap()
    });
    let on = lamp_voice::made_skill::put_on_within(
        &keeping.directory,
        &socket,
        &made.sha256,
        now_ms(),
        std::time::Duration::from_secs(60),
    );
    assert!(helper.join().unwrap(), "the helper put it on");
    assert_eq!(on.as_deref(), Ok("evening-dim"));
    assert!(skills.join("evening-dim").join("skill.toml").is_file());
    // On, it is put on by nobody again: the helper refuses it, and the
    // voice asks no helper.
    let refused = install_made(
        &skills,
        None,
        &keeping.directory,
        &made.sha256,
        std::time::Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(refused.contains("didn't mark that made skill"), "{refused}");
    let again = lamp_voice::made_skill::put_on_within(
        &keeping.directory,
        &dir.path().join("nobody.sock"),
        &made.sha256,
        now_ms(),
        std::time::Duration::from_secs(1),
    );
    assert_eq!(again.as_deref(), Ok("evening-dim"));
}

/// The manage helper puts a made skill on only as the voice's own
/// `put_on` marks it: within `GOES_ON_WITHIN` of its mark, by the helper's
/// clock, and not on already. A kept one never marked, one marked too long
/// ago, or one marked after the helper's now (a clock gone back), puts
/// nothing on, whoever asks; another's mark lets only that one on.
#[test]
fn a_made_skill_goes_on_only_within_its_mark() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let wait = std::time::Duration::from_millis(200);
    let first = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    let second = keep(&answer(TOML, "morning-glow"), false, "s1:8:1", &keeping).unwrap();
    let not_marked = |refused: String| {
        assert!(
            refused.contains("didn't mark that made skill to go on"),
            "{refused}"
        );
    };
    let install = |made: &Made, now: u64| {
        install_made_at(&skills, None, &keeping.directory, &made.sha256, wait, now)
    };
    // Kept, never marked.
    not_marked(install(&first, 10_000).unwrap_err());
    // Marked, then too long ago.
    goes_on(&keeping.directory, &first.sha256, 10_000).unwrap();
    let within = GOES_ON_WITHIN.as_millis() as u64;
    not_marked(install(&first, 10_000 + within + 1).unwrap_err());
    // Marked after the helper's now: a clock gone back.
    not_marked(install(&first, 9_999).unwrap_err());
    // Another marked once the first's mark ran out: that one alone goes on.
    let later = 10_000 + within + 1_000;
    goes_on(&keeping.directory, &second.sha256, later).unwrap();
    not_marked(install(&first, later + 1).unwrap_err());
    assert!(!skills.join("evening-dim").exists());
    let done = install(&second, later + 1).unwrap();
    assert!(done.contains("morning-glow"), "{done}");
    // Put on, as the voice marks it after the helper's done: never again.
    let index = keeping.directory.join(lamp_voice::made_skill::INDEX);
    let mut entries: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
    for entry in &mut entries {
        if entry["sha256"] == second.sha256.as_str() {
            entry["on"] = serde_json::json!(true);
        }
    }
    fs::write(&index, serde_json::to_vec(&entries).unwrap()).unwrap();
    not_marked(install(&second, later + 2).unwrap_err());
}

/// A skill added from a file (`lamp_voice::upload_skill`): a bare SKILL.md
/// becomes a written skill the real `lamp-skills pack` takes, trigger-less
/// and asked for by its function alone; the helper puts it on only once
/// the voice marked it to go on, held to the added cap, and never in another skill's place,
/// a made one's neither.
#[test]
fn a_skill_added_from_a_file_is_checked_and_never_takes_a_place() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let place = lamp_voice::upload_skill::Place {
        skills: skills.clone(),
        pushed: dir.path().join("pushed"),
    };
    let card = "---\nname: tea-tip\ndescription: Call when someone asks for a tea tip.\n---\n\n# Tea tip\n\nGive one short tip about brewing green tea.\n";
    let made = lamp_voice::upload_skill::take(&place, &keeping, None, card, now_ms()).unwrap();
    let bytes = String::from_utf8_lossy(&fs::read(&made.package).unwrap()).into_owned();
    assert!(bytes.contains("made: here") && bytes.contains("added: file"));
    let wait = std::time::Duration::from_millis(200);
    // Not marked to go on yet: nothing goes on.
    let refused = install_made(&skills, None, &keeping.directory, &made.sha256, wait).unwrap_err();
    assert!(refused.contains("didn't mark"), "{refused}");
    mark(&keeping, &made.sha256);
    install_made(&skills, None, &keeping.directory, &made.sha256, wait).unwrap();
    let toml = fs::read_to_string(skills.join("tea-tip").join("skill.toml")).unwrap();
    assert!(
        toml.contains("tool = \"tea_tip\"") && !toml.contains("[trigger]"),
        "{toml}"
    );
    // A made skill of the same name, kept and put on: an added one
    // in its place is refused, and the other way the made one is not
    // replaced by an added one either.
    let made_dim = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    mark(&keeping, &made_dim.sha256);
    install_made(&skills, None, &keeping.directory, &made_dim.sha256, wait).unwrap();
    let folder = dir.path().join("src").join("evening-dim");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("skill.toml"),
        "kind = \"installable\"\nversion = \"1.0.0\"\ntool = \"evening_dim\"\n[permissions]\nlight = true\n[[do]]\nlight = { dim = 1 }\n",
    )
    .unwrap();
    fs::write(
        folder.join("SKILL.md"),
        "---\nname: evening-dim\ndescription: Dims.\nmade: here\nadded: file\n---\n",
    )
    .unwrap();
    let packed = package::pack(&folder, &keeping.directory).unwrap();
    let sha256 = package::sha256_hex(&fs::read(&packed).unwrap());
    let index = keeping.directory.join(lamp_voice::made_skill::INDEX);
    let mut entries: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
    entries.push(serde_json::json!({
        "sha256": sha256,
        "package": packed.file_name().unwrap().to_str().unwrap(),
    }));
    fs::write(&index, serde_json::to_vec(&entries).unwrap()).unwrap();
    mark(&keeping, &sha256);
    let refused = install_made(&skills, None, &keeping.directory, &sha256, wait).unwrap_err();
    assert!(refused.contains("never takes its place"), "{refused}");
    assert!(
        !fs::read_to_string(skills.join("evening-dim").join("SKILL.md"))
            .unwrap()
            .contains("added: file")
    );
}

/// A skill written in the app (`LAMP_SKILL_WRITTEN`), its files written by
/// the voice from the person's words: the real checker packs it, it is put
/// on only once the voice marked it to go on, and then it loads as a function for the
/// model with no trigger, described with what it does, its steps nothing
/// but the answer.
#[test]
fn a_written_skill_passes_the_real_checker_and_is_put_on_only_once_marked() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let instructions = "Dim the light to level 2.\nSay \"focus time\" once, then stay quiet.";
    let written =
        lamp_voice::made_skill::Written::new("study-mode", "when I say study mode", instructions)
            .unwrap();
    let made = lamp_voice::made_skill::keep_written(&written, "written:app:1:1", &keeping, &skills)
        .unwrap();
    assert_eq!(made.uses.spoken(), "its voice");
    let bytes = fs::read(&made.package).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("written: app"), "marked written in the app");
    assert!(
        text.contains(&format!("---\n{instructions}\n")),
        "the text as written"
    );
    let wait = std::time::Duration::from_millis(200);
    let refused = install_made_at(
        &skills,
        None,
        &keeping.directory,
        &made.sha256,
        wait,
        now_ms(),
    )
    .unwrap_err();
    assert!(
        refused.contains("didn't mark that made skill to go on"),
        "{refused}"
    );
    assert!(
        !skills.join("study-mode").exists(),
        "never declared before it is marked to go on"
    );
    mark(&keeping, &made.sha256);
    install_made_at(
        &skills,
        None,
        &keeping.directory,
        &made.sha256,
        wait,
        now_ms(),
    )
    .unwrap();
    let skill = lamp_skills::load(&skills.join("study-mode")).unwrap();
    assert_eq!(skill.tool.as_deref(), Some("written_study_mode"));
    assert!(skill.triggers.is_empty() && skill.policy.is_empty());
    assert!(
        skill.description.contains("when I say study mode"),
        "{}",
        skill.description
    );
    assert!(skill.permissions.speak && !skill.permissions.light && !skill.permissions.body);
    let tools = lamp_skills::tools_file(std::slice::from_ref(&skill));
    assert_eq!(tools["tools"][0]["name"], "written_study_mode", "{tools}");
}

/// A made skill on `trigger` with one body `step`, reading presence and
/// the clock, as the voice's model writes one.
fn made_body(trigger: &str, step: &str) -> String {
    format!(
        "kind = \"installable\"\nversion = \"1.0.0\"\nneeds = [\"body\"]\n\n[permissions]\n\
         topics = [\"humans/faces/tracked\", \"room/stood_up\", \"clock/local\"]\nbody = true\n\n\
         [trigger]\n{trigger}\n[ask]\npolicy = \"nudge\"\n\n[[do]]\n{step}\n"
    )
}

/// The made skill's topics hold lamp-skills' own: the faces and getting up
/// (`PRESENCE_TOPICS`) and the local clock lamp-skills makes, which the
/// voice can't name and writes out (#6070, the Reviewer-lead's P3).
#[test]
fn a_made_skills_clock_and_presence_are_lamp_skills_own() {
    use lamp_voice::made_skill::{CLOCK_TOPIC, TOPICS};
    assert_eq!(CLOCK_TOPIC, lamp_skills::LOCAL_CLOCK_TOPIC);
    assert!(TOPICS.contains(&lamp_skills::LOCAL_CLOCK_TOPIC));
    for topic in lamp_skills::PRESENCE_TOPICS {
        assert!(TOPICS.contains(&topic), "{topic}");
    }
}

/// A face, a getting up and a time each day, each made by voice, paced and
/// passed by the real `lamp-skills pack` and kept (#6070, the
/// Reviewer-lead's P3): said as lamp-skills will run them.
#[test]
fn a_face_a_getting_up_and_a_clock_made_skill_are_packed_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    for (name, trigger, step, said) in [
        (
            "face-hello",
            "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\n",
            "body = { gesture = \"greeting\" }",
            "When the lamp sees a face, after seeing none for a minute, it lifts its head and \
             arm in a greeting.",
        ),
        (
            "stand-shake",
            "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n",
            "body = { gesture = \"headshake\" }",
            "When someone sitting at the desk gets up, it shakes its head, at most once a \
             minute.",
        ),
        (
            "night-droop",
            "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
             during = { from = \"23:59\", to = \"00:00\" }\n",
            "body = { gesture = \"sleepy\" }",
            "Every day at 23:59, it droops sleepily.",
        ),
    ] {
        let toml = made_body(trigger, step);
        let made = keep(
            &answer(&toml, name),
            false,
            &format!("s1:{name}:1"),
            &keeping,
        )
        .unwrap_or_else(|refused| panic!("{name}: {refused:?}"));
        assert_eq!(made.name, name);
        assert_eq!(made.description, said, "{name}");
        assert!(made.uses.body, "{name}");
    }
    assert_eq!(tars(&keeping.directory).len(), 3);
}

/// #6070, the Reviewer-lead's P2: a made skill on a face coming into view
/// that only moves, made by voice with no `absent_for`, kept, put on and
/// run by the real engine. Someone at the desk who glances away for a frame
/// is not greeted again; back after more than a minute away, they are.
/// Unpaced, the glance would greet them again (CLAUDE.md rule 8).
#[test]
fn a_made_face_skill_greets_once_through_a_glance_away() {
    const S: u64 = 1_000_000;
    const FACE: &[u8] = br#"{"faces":[{"id":1,"xmin":0.4,"xmax":0.6,"ymin":0.3,"ymax":0.55}]}"#;
    const NOBODY: &[u8] = br#"{"faces":[]}"#;
    let toml = made_body(
        "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\n",
        "body = { gesture = \"greeting\" }",
    );
    assert!(!toml.contains("absent_for"));
    let greetings = |skill: lamp_skills::Skill| {
        let mut engine = lamp_skills::Engine::new(vec![skill], true);
        let mut at_s = Vec::new();
        for at in 0..260u64 {
            let faces = match at {
                0..70 => NOBODY,
                // A frame without the face: a glance away.
                100 => NOBODY,
                70..130 => FACE,
                // Away 70 s, then back.
                130..200 => NOBODY,
                _ => FACE,
            };
            let mut runs = engine.tick(at * S);
            runs.extend(engine.observe("humans/faces/tracked", at * S, faces, Some(2_000)));
            if runs.iter().any(|run| {
                run.events
                    .iter()
                    .any(|event| event.stage == lamp_msgs::SkillStage::Triggered)
            }) {
                at_s.push(at);
            }
        }
        at_s
    };
    // Made by voice: paced, packed, kept, marked and put on, then loaded as
    // lamp-skills loads what is on.
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let made = keep(&answer(&toml, "face-hello"), false, "s1:face:1", &keeping).unwrap();
    let now = now_ms();
    goes_on(&keeping.directory, &made.sha256, now).unwrap();
    install_made_at(
        &skills,
        None,
        &keeping.directory,
        &made.sha256,
        std::time::Duration::from_millis(200),
        now + 1,
    )
    .unwrap();
    let on = skills.join("face-hello");
    assert!(
        fs::read_to_string(on.join("skill.toml"))
            .unwrap()
            .contains("absent_for = \"1m\"")
    );
    assert_eq!(greetings(lamp_skills::load(&on).unwrap()), [70, 200]);
    // Unpaced, the same skill greets the glance back at 101 s too.
    let unpaced = dir.path().join("unpaced").join("face-hello");
    fs::create_dir_all(&unpaced).unwrap();
    fs::write(unpaced.join("skill.toml"), &toml).unwrap();
    fs::write(unpaced.join("SKILL.md"), card("face-hello")).unwrap();
    assert_eq!(
        greetings(lamp_skills::load(&unpaced).unwrap()),
        [70, 101, 200]
    );
}
