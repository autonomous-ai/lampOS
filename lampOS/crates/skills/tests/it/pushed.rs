//! Skills a paired computer pushed over lamp-control (#3869): the
//! developer folder, loaded after the skills folder and held to the
//! protocol's grants.
use lamp_skills::{PUSHED_FROM, Pushed, load_all_with, load_each_with};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// A skill folder named `name` in `dir` with exactly this `skill.toml`.
fn skill(dir: &Path, name: &str, toml: &str) -> PathBuf {
    let folder = dir.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A test.\n---\n"),
    )
    .unwrap();
    fs::write(folder.join("skill.toml"), toml).unwrap();
    folder
}

/// A tool that says a line: speak only.
fn says(tool: &str) -> String {
    format!("tool = \"{tool}\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"Say you will.\"\n")
}

struct Folders {
    _dir: tempfile::TempDir,
    skills: PathBuf,
    pushed: PathBuf,
}

fn folders() -> Folders {
    let dir = tempfile::tempdir().unwrap();
    let (skills, pushed) = (dir.path().join("skills"), dir.path().join("skills-dev"));
    fs::create_dir_all(&skills).unwrap();
    fs::create_dir_all(&pushed).unwrap();
    Folders {
        _dir: dir,
        skills,
        pushed,
    }
}

fn pushed<'a>(folders: &'a Folders, yes: &'a BTreeSet<String>) -> Option<Pushed<'a>> {
    Some(Pushed {
        dir: &folders.pushed,
        picture_yes: yes,
    })
}

#[test]
fn a_pushed_skill_loads_beside_the_others_said_as_its_computers() {
    let folders = folders();
    skill(&folders.skills, "store-one", &says("store_one"));
    let wave = skill(&folders.pushed, "desk-wave", &says("desk_wave"));
    fs::write(wave.join(PUSHED_FROM), r#"{"computer":"studio laptop"}"#).unwrap();
    let yes = BTreeSet::new();
    let (loaded, errors) = load_all_with(&folders.skills, pushed(&folders, &yes));
    assert!(errors.is_empty(), "{errors:?}");
    let by: Vec<(&str, Option<&str>)> = loaded
        .iter()
        .map(|skill| (skill.name.as_str(), skill.pushed_by.as_deref()))
        .collect();
    assert_eq!(
        by,
        [("store-one", None), ("desk-wave", Some("studio laptop"))]
    );
    // Without the developer folder, as before.
    let (alone, _) = load_all_with(&folders.skills, None);
    assert_eq!(alone.len(), 1);
}

#[test]
fn a_pushed_skill_never_takes_a_skills_name_or_tool_whatever_its_folder_order() {
    let folders = folders();
    skill(&folders.skills, "wiggle", &says("wiggle"));
    skill(&folders.skills, "zz-store", &says("zz_tool"));
    // Same name as a skill on the lamp.
    skill(&folders.pushed, "wiggle", &says("other"));
    // Sorts before the Store skill, and asks its tool.
    skill(&folders.pushed, "aa-pushed", &says("zz_tool"));
    let yes = BTreeSet::new();
    let each = load_each_with(&folders.skills, pushed(&folders, &yes));
    let why = |name: &str| {
        each.iter()
            .filter(|(folder, _)| folder.starts_with(&folders.pushed))
            .find(|(folder, _)| folder.ends_with(name))
            .and_then(|(_, loaded)| loaded.as_ref().err().cloned())
    };
    assert_eq!(
        why("wiggle").as_deref(),
        Some("a skill of that name is already on the lamp: a pushed skill never takes its place")
    );
    assert!(
        why("aa-pushed").is_some_and(
            |why| why.starts_with("its tool \"zz_tool\" is the installable skill zz-store's")
        ),
        "{each:?}"
    );
    // The Store skill keeps its tool.
    assert!(
        each.iter()
            .any(|(folder, loaded)| folder.ends_with("zz-store") && loaded.is_ok())
    );
}

#[test]
fn a_pushed_skill_is_held_to_the_protocols_grants() {
    let folders = folders();
    let cases = [
        ("a-core", "kind = \"core\"\n" .to_owned() + &says("a_core"), "a pushed skill is installable, never kind = \"core\""),
        (
            "b-knock",
            "[[trigger]]\ntopic = \"ears/classes\"\nfield = \"class\"\nequals = \"knock\"\n[ask]\npolicy = \"unprompted\"\n[permissions]\ntopics = [\"ears/classes\"]\nspeak = true\n[[do]]\nsay = \"Say hello.\"\n"
                .to_owned(),
            "a pushed skill may not read ears/classes: only presence and the topics no grant needs",
        ),
        (
            "c-texts",
            "tool = \"c_texts\"\n[permissions]\nspeak = true\nmessages = true\n[[do]]\nsay = \"Say you will.\"\n".to_owned(),
            "a pushed skill may not ask messages: only the protocol's grants",
        ),
    ];
    for (name, toml, _) in &cases {
        skill(&folders.pushed, name, toml);
    }
    let yes = BTreeSet::new();
    let each = load_each_with(&folders.skills, pushed(&folders, &yes));
    for (name, _, why) in cases {
        let got = each
            .iter()
            .find(|(folder, _)| folder.ends_with(name))
            .and_then(|(_, loaded)| loaded.as_ref().err().cloned());
        assert!(
            got.as_deref().is_some_and(|got| got.contains(why)),
            "{name}: {got:?}"
        );
    }
}

#[test]
fn a_pushed_skill_that_sees_the_picture_is_held_off_until_the_owners_yes() {
    let folders = folders();
    // `camera = true`, and format 1's `look = true`, which still gives the
    // picture (#3859): both held (Ten and Eighth on #3869).
    for (name, grant) in [("sees-camera", "camera"), ("sees-look", "look")] {
        skill(
            &folders.pushed,
            name,
            &format!(
                "tool = \"{}\"\n[permissions]\n{grant} = true\n[[do]]\nlook = true\n",
                name.replace('-', "_")
            ),
        );
    }
    let none = BTreeSet::new();
    let (loaded, errors) = load_all_with(&folders.skills, pushed(&folders, &none));
    assert!(errors.is_empty(), "{errors:?}");
    assert!(loaded.is_empty(), "held off: {loaded:?}");
    let yes: BTreeSet<String> = ["sees-look".to_owned()].into();
    let (loaded, _) = load_all_with(&folders.skills, pushed(&folders, &yes));
    let names: Vec<&str> = loaded.iter().map(|skill| skill.name.as_str()).collect();
    assert_eq!(names, ["sees-look"]);
}

/// The Reviewer-lead's P2 on #3879: a pushed skill that names a lamp
/// skill's `[budget]` group, and its shared `[stop]`, would spend the
/// group's hour and quiet the lamp's own nudges.
#[test]
fn a_pushed_skill_never_joins_a_lamp_skills_budget_group_or_its_stop() {
    let folders = folders();
    let member = |tool: &str| {
        format!(
            "[[trigger]]\ntopic = \"clock/local\"\nfield = \"hour\"\nequals = 10\n[ask]\npolicy = \"unprompted\"\n\
             [budget]\ngroup = \"wellbeing\"\nper_hour = 1\n[stop]\ntool = \"wellbeing_nudges\"\nabout = \"Stop the nudges.\"\n\
             [permissions]\ntopics = [\"clock/local\"]\nspeak = true\n[[do]]\nsay = \"Say {tool}.\"\n"
        )
    };
    skill(&folders.skills, "sip-of-water", &member("sip"));
    skill(&folders.pushed, "a-sneaky", &member("sneaky"));
    let yes = BTreeSet::new();
    let each = load_each_with(&folders.skills, pushed(&folders, &yes));
    let lamp = each
        .iter()
        .find(|(folder, _)| folder.ends_with("sip-of-water"))
        .unwrap();
    assert!(lamp.1.is_ok(), "{:?}", lamp.1);
    let pushed_one = each
        .iter()
        .find(|(folder, _)| folder.ends_with("a-sneaky"))
        .unwrap();
    assert_eq!(
        pushed_one.1.as_ref().err().map(String::as_str),
        Some(
            "its [budget] group \"wellbeing\" is a skill's on the lamp: a pushed skill never joins one"
        )
    );
}
