//! A skill made by voice (#1685, `lamp_voice::made_skill`), checked by the
//! real `lamp-skills pack`, the checker an install uses (#1698): what passes
//! is packed and kept in a 0700 folder with its SHA-256, at most five;
//! what does more than it declares, or more than a made skill may, is not.
use lamp_skills::package::{self, asked as asked_helper, install_made};
use lamp_voice::made_skill::{KEPT_MOST, Keeping, Made, Refused, keep};
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

[quiet]
from = "22:30"
to = "22:00"

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
    }
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
    assert_eq!(made.description, "Dims the light a shade at 10 pm.");
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
    let refused = keep(&answer(&gesture, "evening-dim"), false, "s1:8:1", &keeping);
    assert_eq!(
        refused,
        Err(Refused("it doesn't pass the lamp's check".into()))
    );
    let speaking = TOML.replace("light = true\n", "light = true\nspeak = true\n");
    let capped = keep(&answer(&speaking, "evening-dim"), false, "s1:9:1", &keeping);
    assert!(capped.unwrap_err().0.contains("more than the light"));
    assert!(tars(&keeping.directory).is_empty());
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

/// A made skill put on at its person's yes, bound to its package's SHA-256
/// (the lead, 20 Oct 08:37 on #1711): the manage helper's `install_made`
/// takes the package the voice kept with that hash, and installs it.
#[test]
fn a_made_skill_goes_on_by_its_sha256_and_only_by_it() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let made = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    let wait = std::time::Duration::from_millis(200);
    // Another hash puts nothing on.
    let other = "0".repeat(64);
    let refused = install_made(&skills, &keeping.directory, &other, wait).unwrap_err();
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
        wait,
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
        done.contains("bound to the person's yes by its SHA-256") && !done.contains("signature"),
        "{done}"
    );
    assert!(skills.join("evening-dim").join("skill.toml").is_file());
    assert!(
        fs::read_to_string(skills.join("evening-dim").join("SKILL.md"))
            .unwrap()
            .contains("made: here")
    );
}

/// The bytes put on are the bytes the yes was for: a package changed after
/// it was kept is refused, and nothing goes on.
#[test]
fn a_made_skill_whose_package_changed_does_not_go_on() {
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let made = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    let mut bytes = fs::read(&made.package).unwrap();
    bytes.push(0);
    fs::write(&made.package, &bytes).unwrap();
    let refused = install_made(
        &skills,
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
            "more than the light",
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
        let refused = install_made(
            &skills,
            &keeping.directory,
            &sha256,
            std::time::Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(refused.contains(why), "{name}: {refused}");
        assert!(!skills.join(name).exists(), "{name}");
    }
}

/// A person's yes to their own made skill never takes the place of a skill
/// they put on another way (the reviewer's P3 on #2414): one of the same
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
    package::install(&skills, &store).unwrap();
    let made = keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    let wait = std::time::Duration::from_millis(200);
    let refused = install_made(&skills, &keeping.directory, &made.sha256, wait).unwrap_err();
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
    package::remove(&skills, "evening-dim").unwrap();
    install_made(&skills, &keeping.directory, &made.sha256, wait).unwrap();
    let again = keep(
        &answer(&TOML.replace("1.0.0", "1.0.1"), "evening-dim"),
        false,
        "s1:8:1",
        &keeping,
    )
    .unwrap();
    let replaced = install_made(&skills, &keeping.directory, &again.sha256, wait).unwrap();
    assert!(
        replaced.starts_with("replaced evening-dim 1.0.0 with 1.0.1"),
        "{replaced}"
    );
}

/// End to end: the lamp makes a skill, asks to put it on, and on a spoken
/// yes the voice's `put_on` asks the real manage helper, which puts that
/// package on. The helper's socket is the one the voice dials.
#[test]
fn a_made_skill_is_put_on_by_a_spoken_yes_through_the_helper() {
    assert_eq!(
        lamp_voice::made_skill::MANAGE_SOCKET,
        package::MANAGE_SOCKET
    );
    let dir = tempfile::tempdir().unwrap();
    let keeping = keeping(dir.path());
    let skills = dir.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    keep(&answer(TOML, "evening-dim"), false, "s1:7:1", &keeping).unwrap();
    lamp_voice::made_skill::asked(&keeping.directory, "s1:7:1", 1_000).unwrap();
    let socket = dir.path().join("manage.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let (served, helper_made) = (skills.clone(), keeping.directory.clone());
    let helper = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        asked_helper(
            &served,
            &helper_made,
            std::time::Duration::from_secs(2),
            &stream,
            &stream,
        )
        .unwrap()
    });
    let told = lamp_voice::made_skill::put_on_within(
        &keeping.directory,
        &socket,
        Some("Yes, please."),
        2_000,
        std::time::Duration::from_secs(60),
    );
    assert!(helper.join().unwrap(), "the helper put it on");
    assert_eq!(told["put_on"], "evening-dim", "{told}");
    assert!(skills.join("evening-dim").join("skill.toml").is_file());
}
