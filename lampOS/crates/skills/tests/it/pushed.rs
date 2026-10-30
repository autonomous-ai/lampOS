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
fn a_pushed_skill_never_joins_a_lamp_skills_budget_group() {
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

/// The Reviewer-lead's P3 on #3879: a lamp skill that fails to load just
/// then still holds its `[budget]` group from a push.
#[test]
fn a_lamp_skill_that_fails_to_load_still_holds_its_budget_group() {
    let folders = folders();
    let broken = skill(
        &folders.skills,
        "sip-of-water",
        "[budget]\ngroup = \"wellbeing\"\nper_hour = 1\nnot_a_key = 1\n",
    );
    assert!(broken.exists());
    skill(
        &folders.pushed,
        "a-sneaky",
        "[[trigger]]\ntopic = \"clock/local\"\nfield = \"hour\"\nequals = 10\n[ask]\npolicy = \"unprompted\"\n\
         [budget]\ngroup = \"wellbeing\"\nper_hour = 1\n[permissions]\ntopics = [\"clock/local\"]\nspeak = true\n[[do]]\nsay = \"Say hi.\"\n",
    );
    let yes = BTreeSet::new();
    let each = load_each_with(&folders.skills, pushed(&folders, &yes));
    assert!(
        each.iter()
            .find(|(folder, _)| folder.ends_with("sip-of-water"))
            .unwrap()
            .1
            .is_err()
    );
    assert_eq!(
        each.iter()
            .find(|(folder, _)| folder.ends_with("a-sneaky"))
            .and_then(|(_, loaded)| loaded.as_ref().err().cloned())
            .as_deref(),
        Some(
            "its [budget] group \"wellbeing\" is a skill's on the lamp: a pushed skill never joins one"
        )
    );
}

/// Piece B: the manage helper's `install_pushed`, as `lamp-control` keeps
/// the push in flight (`push/push.json` beside `push/package.tar`).
mod install {
    use super::*;
    use lamp_skills::package::{
        HELD_FOR_THE_PICTURE, PUSH_INDEX, PUSH_PACKAGE, install_pushed_at, pack, pushed_from,
        sha256_hex,
    };
    use std::time::Duration;

    const NOW: u64 = 1_761_600_000_000;

    struct Lamp {
        folders: Folders,
        push: PathBuf,
        work: tempfile::TempDir,
    }

    fn lamp() -> Lamp {
        let folders = folders();
        let push = folders.pushed.parent().unwrap().join("push");
        fs::create_dir_all(&push).unwrap();
        Lamp {
            folders,
            push,
            work: tempfile::tempdir().unwrap(),
        }
    }

    /// `toml` packed as `name` 0.1.0, kept as the push in flight from
    /// `client` with `grants`, taken at `at`: its SHA-256.
    fn kept(lamp: &Lamp, name: &str, toml: &str, client: &str, grants: &[&str], at: u64) -> String {
        let source = lamp.work.path().join("src");
        let _ = fs::remove_dir_all(&source);
        skill(&source, name, &format!("version = \"0.1.0\"\n{toml}"));
        let out = lamp.work.path().join("out");
        let packed = pack(&source.join(name), &out).unwrap();
        let bytes = fs::read(&packed).unwrap();
        let sha = sha256_hex(&bytes);
        fs::write(lamp.push.join(PUSH_PACKAGE), &bytes).unwrap();
        fs::write(
            lamp.push.join(PUSH_INDEX),
            serde_json::json!({
                "sha256": sha, "client": client, "client_name": format!("{client}'s laptop"),
                "name": name, "grants": grants, "at_unix_ms": at,
            })
            .to_string(),
        )
        .unwrap();
        sha
    }

    fn install(lamp: &Lamp, sha: &str) -> Result<lamp_skills::package::PushedDone, String> {
        install_pushed_at(
            &lamp.folders.skills,
            &lamp.folders.pushed,
            &lamp.push,
            &lamp.work.path().join("store-key.pub"),
            sha,
            Duration::from_secs(2),
            NOW,
        )
    }

    #[test]
    fn a_push_is_installed_in_the_developer_folder_said_as_its_computers() {
        let lamp = lamp();
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            NOW - 1_000,
        );
        let done = install(&lamp, &sha).unwrap();
        assert_eq!(
            (
                done.name.as_str(),
                done.on,
                done.held.as_deref(),
                done.asks.as_slice(),
                done.shown.as_slice()
            ),
            (
                "desk-wave",
                true,
                None,
                ["speak".to_owned()].as_slice(),
                ["speaks and plays sounds".to_owned()].as_slice()
            )
        );
        let folder = lamp.folders.pushed.join("desk-wave");
        let from = pushed_from(&folder).unwrap();
        assert_eq!(
            (
                from.client.as_str(),
                from.computer.as_str(),
                from.sha256.as_str()
            ),
            ("c_1", "c_1's laptop", sha.as_str())
        );
        let yes = BTreeSet::new();
        let (loaded, _) = load_all_with(&lamp.folders.skills, pushed(&lamp.folders, &yes));
        assert_eq!(loaded[0].pushed_by.as_deref(), Some("c_1's laptop"));
    }

    #[test]
    fn only_the_push_kept_within_its_minute_is_installed() {
        let lamp = lamp();
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            NOW - 61_000,
        );
        assert!(
            install(&lamp, &sha)
                .unwrap_err()
                .contains("within the last 60 s")
        );
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            NOW + 1,
        );
        assert!(
            install(&lamp, &sha)
                .unwrap_err()
                .contains("within the last 60 s")
        );
        kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            NOW,
        );
        assert_eq!(
            install(&lamp, &"0".repeat(64)).unwrap_err(),
            "not installed: the push kept is not that SHA-256"
        );
        assert!(!lamp.folders.pushed.join("desk-wave").exists());
    }

    #[test]
    fn a_push_is_held_to_its_computers_grants_and_the_picture_to_the_owners_yes() {
        let lamp = lamp();
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["light"],
            NOW,
        );
        assert_eq!(
            install(&lamp, &sha).unwrap_err(),
            "not installed: it asks speak, which this computer was not granted"
        );
        let sha = kept(
            &lamp,
            "sees-desk",
            "tool = \"sees_desk\"\n[permissions]\ncamera = true\n[[do]]\nlook = true\n",
            "c_1",
            &["body"],
            NOW,
        );
        let done = install(&lamp, &sha).unwrap();
        assert_eq!(
            (done.on, done.held.as_deref()),
            (false, Some(HELD_FOR_THE_PICTURE))
        );
        assert_eq!(done.asks, ["camera"]);
    }

    #[test]
    fn a_push_never_takes_a_lamp_skills_or_another_computers_place() {
        let lamp = lamp();
        skill(&lamp.folders.skills, "wiggle", &says("wiggle"));
        let sha = kept(&lamp, "wiggle", &says("other"), "c_1", &["speak"], NOW);
        assert!(
            install(&lamp, &sha)
                .unwrap_err()
                .contains("a skill of that name is already on the lamp")
        );
        let sha = kept(&lamp, "a-wave", &says("wiggle"), "c_1", &["speak"], NOW);
        assert!(
            install(&lamp, &sha)
                .unwrap_err()
                .contains("its tool \"wiggle\" is the installable skill wiggle's")
        );
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            NOW,
        );
        install(&lamp, &sha).unwrap();
        // Another computer, the same name: refused. The same one: replaced.
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_2",
            &["speak"],
            NOW,
        );
        assert_eq!(
            install(&lamp, &sha).unwrap_err(),
            "not installed: another computer's skill has that name"
        );
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave_two"),
            "c_1",
            &["speak"],
            NOW,
        );
        install(&lamp, &sha).unwrap();
        assert_eq!(
            pushed_from(&lamp.folders.pushed.join("desk-wave"))
                .unwrap()
                .sha256,
            sha
        );
    }

    /// One line asked of the manage helper, as lamp-control asks it.
    fn ask(lamp: &Lamp, state: &Path, line: &str) -> serde_json::Value {
        ask_as(lamp, state, line, lamp_skills::package::Asker::Control)
    }

    /// One line asked of the manage helper by `asker`.
    fn ask_as(
        lamp: &Lamp,
        state: &Path,
        line: &str,
        asker: lamp_skills::package::Asker,
    ) -> serde_json::Value {
        let mut answer = Vec::new();
        lamp_skills::package::asked(
            &lamp.folders.skills,
            &lamp.folders.skills.join("no-made"),
            state,
            Duration::from_secs(2),
            None,
            Some(lamp_skills::package::Pushes {
                dev: &lamp.folders.pushed,
                push: &lamp.push,
                asker,
                key_file: &lamp.work.path().join("store-key.pub"),
            }),
            format!("{line}\n").as_bytes(),
            &mut answer,
        )
        .unwrap();
        serde_json::from_slice(&answer).unwrap()
    }

    #[test]
    fn the_helper_lists_turns_off_and_removes_a_computers_own_pushes() {
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        // install_pushed is the push kept now, asked by its SHA-256.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            now,
        );
        let done = ask(&lamp, &state, &format!("{{\"install_pushed\": \"{sha}\"}}"));
        assert_eq!(done["done"]["name"], "desk-wave", "{done}");
        assert_eq!(done["done"]["on"], true);
        let listed = ask(&lamp, &state, r#"{"list_pushed_by": "c_1"}"#);
        assert_eq!(listed["done"][0]["name"], "desk-wave", "{listed}");
        assert_eq!(listed["done"][0]["sha256"], sha);
        // As control.md shows it: a time in UTC, and the list itself, which
        // lamp-control answers as `skills` (lamp-75c9, 30 Oct 13:23).
        let at = listed["done"][0]["pushed_at"].as_str().unwrap();
        assert!(
            at.len() == 20 && at.ends_with('Z') && at.as_bytes()[10] == b'T',
            "{at}"
        );
        assert_eq!(
            ask(&lamp, &state, r#"{"list_pushed_by": "c_2"}"#)["done"],
            serde_json::json!([])
        );
        // Another computer can't remove it.
        let refused = ask(
            &lamp,
            &state,
            r#"{"remove_pushed": {"client": "c_2", "name": "desk-wave"}}"#,
        );
        assert_eq!(
            refused["refused"],
            "not removed: another computer pushed that skill"
        );
        // A revoke turns it off, kept.
        let off = ask(&lamp, &state, r#"{"off_pushed_by": "c_1"}"#);
        assert_eq!(
            off["done"]["off"],
            serde_json::json!(["desk-wave"]),
            "{off}"
        );
        assert_eq!(
            ask(&lamp, &state, r#"{"list_pushed_by": "c_1"}"#)["done"][0]["on"],
            false
        );
        assert!(lamp.folders.pushed.join("desk-wave").exists());
        // Its own computer removes it.
        let removed = ask(
            &lamp,
            &state,
            r#"{"remove_pushed": {"client": "c_1", "name": "desk-wave"}}"#,
        );
        assert!(removed.get("done").is_some(), "{removed}");
        assert!(!lamp.folders.pushed.join("desk-wave").exists());
        // A bad id is refused, nothing done.
        assert!(ask(&lamp, &state, r#"{"off_pushed_by": "../x"}"#)["refused"].is_string());
    }

    /// Fourteen's P2 on #3910: a helper serves each connection on its own,
    /// so a revoke may be served before an install already asked. The
    /// revoke is kept first, so the push it came after never lands.
    #[test]
    fn a_revoke_served_before_the_install_still_stops_that_push() {
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            NOW - 1_000,
        );
        let off = lamp_skills::package::off_pushed_by_at(
            &lamp.folders.pushed,
            &state,
            "c_1",
            None,
            Duration::from_secs(2),
            NOW - 500,
        )
        .unwrap();
        assert_eq!(
            off,
            lamp_skills::package::Changed::Off(Vec::new()),
            "nothing of c_1's was on yet"
        );
        assert_eq!(
            install(&lamp, &sha).err().as_deref(),
            Some("not installed: that computer was revoked after the push")
        );
        assert!(!lamp.folders.pushed.join("desk-wave").exists());
        // Another computer's push, and c_1's own after it paired again, install.
        let other = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_2",
            &["speak"],
            NOW - 1_000,
        );
        assert!(install(&lamp, &other).is_ok());
        let again = kept(
            &lamp,
            "desk-nod",
            &says("desk_nod"),
            "c_1",
            &["speak"],
            NOW - 100,
        );
        assert!(install(&lamp, &again).is_ok());
    }

    /// Reviewer-two's P2 on #3910: a re-pair with fewer grants turns off
    /// what asks more, and a push made before it under the old grants never
    /// lands; a revoke before it still holds for a push made before that.
    #[test]
    fn a_re_pair_with_fewer_grants_turns_off_what_asks_more_and_stops_older_pushes() {
        use lamp_skills::package::{grants_pushed_by_at, off_pushed_by_at};
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let lights = "tool = \"desk_glow\"\n[permissions]\nlight = true\n[[do]]\nlight = { colour = \"warm\" }\n";
        for (name, toml) in [
            ("desk-wave", says("desk_wave")),
            ("desk-glow", lights.to_owned()),
        ] {
            let sha = kept(&lamp, name, &toml, "c_1", &["speak", "light"], NOW - 5_000);
            install(&lamp, &sha).unwrap();
        }
        let speak = vec!["speak".to_owned()];
        let off = grants_pushed_by_at(
            &lamp.folders.pushed,
            &state,
            "c_1",
            &speak,
            None,
            Duration::from_secs(2),
            NOW - 4_000,
        )
        .unwrap();
        assert_eq!(
            off,
            lamp_skills::package::Changed::Off(vec!["desk-glow".to_owned()])
        );
        // A push made before it under the old grants.
        let late = kept(
            &lamp,
            "desk-glow",
            lights,
            "c_1",
            &["speak", "light"],
            NOW - 4_500,
        );
        assert_eq!(
            install(&lamp, &late).err().as_deref(),
            Some("not installed: it asks light, which this computer was not granted")
        );
        // One that asks only what it still holds installs.
        let fine = kept(
            &lamp,
            "desk-nod",
            &says("desk_nod"),
            "c_1",
            &["speak", "light"],
            NOW - 4_500,
        );
        assert!(install(&lamp, &fine).is_ok());
        // A revoke, then a re-pair granting all again: a push made before
        // the revoke still never lands.
        off_pushed_by_at(
            &lamp.folders.pushed,
            &state,
            "c_1",
            None,
            Duration::from_secs(2),
            NOW - 3_000,
        )
        .unwrap();
        grants_pushed_by_at(
            &lamp.folders.pushed,
            &state,
            "c_1",
            &["speak".to_owned(), "light".to_owned()],
            None,
            Duration::from_secs(2),
            NOW - 2_000,
        )
        .unwrap();
        let before = kept(
            &lamp,
            "desk-tap",
            &says("desk_tap"),
            "c_1",
            &["speak"],
            NOW - 3_500,
        );
        assert_eq!(
            install(&lamp, &before).err().as_deref(),
            Some("not installed: that computer was revoked after the push")
        );
        let after = kept(
            &lamp,
            "desk-tap",
            &says("desk_tap"),
            "c_1",
            &["speak"],
            NOW - 1_000,
        );
        assert!(install(&lamp, &after).is_ok());
    }

    /// Reviewer-two on #3504: only lamp-control asks the pushed requests,
    /// and it asks nothing else; the app keeps its own, by their units.
    #[test]
    fn only_lamp_control_asks_for_pushed_skills_and_nothing_else() {
        use lamp_skills::package::{Asker, CONTROL, NOT_ITS_REQUEST, asker_of};
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        assert_eq!(
            ask_as(&lamp, &state, r#"{"list_pushed_by": "c_1"}"#, Asker::App)["refused"],
            NOT_ITS_REQUEST
        );
        assert_eq!(
            ask_as(&lamp, &state, r#"{"remove": "wiggle"}"#, Asker::Control)["refused"],
            NOT_ITS_REQUEST
        );
        assert!(ask(&lamp, &state, r#"{"list_pushed_by": "c_1"}"#)["done"].is_array());
        assert_eq!(asker_of(&format!("0::{CONTROL}\n")), Some(Asker::Control));
        assert_eq!(
            asker_of("0::/lampos.slice/lampos-parts.slice/lamp-cloud.service\n"),
            Some(Asker::App)
        );
        assert_eq!(
            asker_of("0::/lampos.slice/lampos-parts.slice/lamp-control.service.d\n"),
            None
        );
    }

    /// Reviewer-two on #3504: the fence fails closed. A record that can't
    /// be read refuses every install, and the next change replaces it with
    /// a revoke of every computer; past the most kept, a floor stands in
    /// for the dropped.
    #[test]
    fn the_change_record_fails_closed() {
        use lamp_skills::package::{REVOKED, grants_pushed_by_at};
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            NOW - 1_000,
        );
        fs::write(lamp.folders.pushed.join(REVOKED), "not json").unwrap();
        assert!(
            install(&lamp, &sha)
                .err()
                .is_some_and(|why| why.starts_with(&format!("not installed: {REVOKED}"))),
        );
        // Another computer's change, now: c_1's older push stays refused.
        let speak = vec!["speak".to_owned()];
        grants_pushed_by_at(
            &lamp.folders.pushed,
            &state,
            "c_2",
            &speak,
            None,
            Duration::from_secs(2),
            NOW - 900,
        )
        .unwrap();
        assert_eq!(
            install(&lamp, &sha).err().as_deref(),
            Some("not installed: that computer was revoked after the push")
        );
        // Seventy changes of c_2's: the floor keeps c_1's push refused.
        fs::remove_file(lamp.folders.pushed.join(REVOKED)).unwrap();
        let sha = kept(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "c_1",
            &["speak"],
            NOW - 1_000,
        );
        for at in 0..70 {
            grants_pushed_by_at(
                &lamp.folders.pushed,
                &state,
                "c_2",
                &speak,
                None,
                Duration::from_secs(2),
                NOW - 900 + at,
            )
            .unwrap();
        }
        assert!(install(&lamp, &sha).is_err());
        let fresh = kept(
            &lamp,
            "desk-nod",
            &says("desk_nod"),
            "c_1",
            &["speak"],
            NOW - 100,
        );
        assert!(install(&lamp, &fresh).is_ok());
    }

    /// Format 2's `look` is presence (#3878): a push of one asks `look`,
    /// held to the computer's grant, and isn't held for the picture.
    #[test]
    fn a_format_2_look_asks_the_look_grant_not_the_picture() {
        let lamp = lamp();
        let toml = "format = 2\ntool = \"desk_look\"\n[permissions]\nlook = true\nspeak = true\n[[do]]\nsay = \"Say you will.\"\n";
        let sha = kept(&lamp, "desk-look", toml, "c_1", &["speak"], NOW - 1_000);
        assert_eq!(
            install(&lamp, &sha).err().as_deref(),
            Some("not installed: it asks look, which this computer was not granted")
        );
        let sha = kept(
            &lamp,
            "desk-look",
            toml,
            "c_1",
            &["speak", "look"],
            NOW - 1_000,
        );
        let done = install(&lamp, &sha).unwrap();
        assert!(done.asks.contains(&"look".to_owned()), "{done:?}");
        assert_eq!((done.on, done.held), (true, None));
    }

    /// Reviewer-two on #3504: changes are ordered by the door's `rev`,
    /// never the helper's clock. An older, wider change sent after a newer,
    /// narrower one is stale and does nothing, however late it comes; a
    /// push is ordered against a change by `rev` too.
    #[test]
    fn changes_go_by_the_doors_revision_and_an_older_one_is_stale() {
        use lamp_skills::package::{Changed, PUSH_INDEX, grants_pushed_by_at};
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let lights = "tool = \"desk_glow\"\n[permissions]\nlight = true\n[[do]]\nlight = { colour = \"warm\" }\n";
        // Pushed under rev 4, with light.
        let sha = kept(
            &lamp,
            "desk-glow",
            lights,
            "c_1",
            &["speak", "light"],
            NOW - 5_000,
        );
        let mut index: serde_json::Value =
            serde_json::from_slice(&fs::read(lamp.push.join(PUSH_INDEX)).unwrap()).unwrap();
        index["rev"] = 4.into();
        fs::write(lamp.push.join(PUSH_INDEX), index.to_string()).unwrap();
        let grant = |grants: &[&str], rev: u64, at: u64| {
            let grants: Vec<String> = grants.iter().map(|grant| (*grant).to_owned()).collect();
            grants_pushed_by_at(
                &lamp.folders.pushed,
                &state,
                "c_1",
                &grants,
                Some(rev),
                Duration::from_secs(2),
                at,
            )
            .unwrap()
        };
        // Rev 6 takes light back; rev 5, wider, arrives after it: stale.
        assert_eq!(grant(&["speak"], 6, NOW - 4_000), Changed::Off(Vec::new()));
        assert_eq!(
            grant(&["speak", "light"], 5, NOW - 3_000),
            Changed::Stale(6)
        );
        // The rev 4 push, though its time is before neither, never lands
        // under the wider grants: rev 6 came after it.
        assert_eq!(
            install(&lamp, &sha).err().as_deref(),
            Some("not installed: it asks light, which this computer was not granted")
        );
        // Ten minutes on, rev 5 is still stale: the highest rev outlasts
        // the changes' two minutes.
        assert_eq!(
            grant(&["speak", "light"], 5, NOW + 600_000),
            Changed::Stale(6)
        );
        // Rev 7 grants light again, and a push made under rev 7 installs.
        assert_eq!(
            grant(&["speak", "light"], 7, NOW + 600_100),
            Changed::Off(Vec::new())
        );
        let sha = kept(
            &lamp,
            "desk-glow",
            lights,
            "c_1",
            &["speak", "light"],
            NOW - 1_000,
        );
        let mut index: serde_json::Value =
            serde_json::from_slice(&fs::read(lamp.push.join(PUSH_INDEX)).unwrap()).unwrap();
        index["rev"] = 7.into();
        fs::write(lamp.push.join(PUSH_INDEX), index.to_string()).unwrap();
        assert!(install(&lamp, &sha).is_ok());
    }

    /// An author's key, from a seed: `which` tells two authors apart.
    fn author(which: u8) -> ring::signature::Ed25519KeyPair {
        ring::signature::Ed25519KeyPair::from_seed_unchecked(&[which; 32]).unwrap()
    }

    fn public(key: &ring::signature::Ed25519KeyPair) -> [u8; 32] {
        use ring::signature::KeyPair;
        key.public_key().as_ref().try_into().unwrap()
    }

    /// The Store's key in these tests, written where `install` reads it.
    fn store_key(lamp: &Lamp, key: &ring::signature::Ed25519KeyPair) {
        use base64::{Engine, engine::general_purpose::STANDARD};
        fs::write(
            lamp.work.path().join("store-key.pub"),
            format!("{}\n", STANDARD.encode(public(key))),
        )
        .unwrap();
    }

    /// As [`kept`], packed and signed by `key` as its author, the push
    /// carrying `asks` as the computer showed them.
    fn kept_signed(
        lamp: &Lamp,
        name: &str,
        toml: &str,
        version: &str,
        key: Option<&ring::signature::Ed25519KeyPair>,
        asks: Option<&[&str]>,
    ) -> String {
        let source = lamp.work.path().join("src");
        let _ = fs::remove_dir_all(&source);
        skill(&source, name, &format!("version = \"{version}\"\n{toml}"));
        let out = lamp.work.path().join("out");
        let packed =
            lamp_skills::package::pack_signed_by(&source.join(name), &out, key, key.is_some())
                .unwrap();
        let bytes = fs::read(&packed).unwrap();
        let sha = sha256_hex(&bytes);
        fs::write(lamp.push.join(PUSH_PACKAGE), &bytes).unwrap();
        let mut index = serde_json::json!({
            "sha256": sha, "client": "c_1", "client_name": "c_1's laptop",
            "name": name, "grants": ["speak", "light", "look", "body"], "at_unix_ms": NOW - 1_000,
        });
        if let Some(asks) = asks {
            index["asks"] = asks.into();
        }
        fs::write(lamp.push.join(PUSH_INDEX), index.to_string()).unwrap();
        sha
    }

    /// #3931, in the helper (the helper's call, 28 Oct): a shared package is
    /// checked against the Store's keys, its `asks` against the lamp's own,
    /// and its name pinned to its author's whole key.
    #[test]
    fn a_shared_push_is_checked_shown_and_pinned_to_its_author() {
        use lamp_skills::package::ASKS_DIFFER;
        let lamp = lamp();
        let (one, two, store) = (author(1), author(2), author(3));
        store_key(&lamp, &store);
        let fingerprint = |key| lamp_update::manifest::fingerprint(&public(key));
        // Signed with the asks shown: installed, said as its author's.
        let sha = kept_signed(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "0.1.0",
            Some(&one),
            Some(&["speak"]),
        );
        let done = install(&lamp, &sha).unwrap();
        assert_eq!(done.author.as_deref(), Some(fingerprint(&one).as_str()));
        let from = pushed_from(&lamp.folders.pushed.join("desk-wave")).unwrap();
        assert_eq!(from.author_fingerprint, done.author);
        assert_eq!(
            from.author.as_deref().map(str::len),
            Some(44),
            "the whole key, base64"
        );
        // The same author's update installs.
        let sha = kept_signed(
            &lamp,
            "desk-wave",
            &says("desk_wave"),
            "0.2.0",
            Some(&one),
            Some(&["speak"]),
        );
        assert!(install(&lamp, &sha).is_ok());
        // Another signer, or none, over it: signer_changed.
        for key in [Some(&two), None] {
            let sha = kept_signed(
                &lamp,
                "desk-wave",
                &says("desk_wave"),
                "0.3.0",
                key,
                Some(&["speak"]),
            );
            let why = install(&lamp, &sha).unwrap_err();
            assert_eq!(
                why,
                format!(
                    "not installed: desk-wave is signed by key {} on this lamp: remove it first",
                    fingerprint(&one)
                )
            );
        }
        // A signed push over a name pushed unsigned.
        let sha = kept_signed(&lamp, "desk-nod", &says("desk_nod"), "0.1.0", None, None);
        install(&lamp, &sha).unwrap();
        let sha = kept_signed(
            &lamp,
            "desk-nod",
            &says("desk_nod"),
            "0.2.0",
            Some(&one),
            Some(&["speak"]),
        );
        assert_eq!(
            install(&lamp, &sha).unwrap_err(),
            "not installed: desk-nod was pushed unsigned on this lamp: remove it first"
        );
        // The asks: missing, or not the lamp's own.
        let sha = kept_signed(
            &lamp,
            "desk-tap",
            &says("desk_tap"),
            "0.1.0",
            Some(&one),
            None,
        );
        assert!(install(&lamp, &sha).unwrap_err().contains(ASKS_DIFFER));
        let sha = kept_signed(
            &lamp,
            "desk-tap",
            &says("desk_tap"),
            "0.1.0",
            Some(&one),
            Some(&["speak", "light"]),
        );
        assert_eq!(
            install(&lamp, &sha).unwrap_err(),
            format!("not installed: {ASKS_DIFFER}: it asks [speak], and [speak, light] were shown")
        );
        // Carrying the Store's key: no author's.
        let sha = kept_signed(
            &lamp,
            "desk-tap",
            &says("desk_tap"),
            "0.1.0",
            Some(&store),
            Some(&["speak"]),
        );
        assert!(
            install(&lamp, &sha)
                .unwrap_err()
                .starts_with("not installed: its signature: ")
        );
        // The Store's key unreadable: refused, never an empty list.
        fs::remove_file(lamp.work.path().join("store-key.pub")).unwrap();
        let sha = kept_signed(
            &lamp,
            "desk-tap",
            &says("desk_tap"),
            "0.1.0",
            Some(&one),
            Some(&["speak"]),
        );
        assert!(
            install(&lamp, &sha)
                .unwrap_err()
                .contains("the Store's key")
        );
    }

    /// The codes the door passes on, by the helper's answer.
    #[test]
    fn a_shared_pushs_refusals_carry_their_codes() {
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        store_key(&lamp, &author(3));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let at_now = |lamp: &Lamp| {
            let mut index: serde_json::Value =
                serde_json::from_slice(&fs::read(lamp.push.join(PUSH_INDEX)).unwrap()).unwrap();
            index["at_unix_ms"] = now.into();
            fs::write(lamp.push.join(PUSH_INDEX), index.to_string()).unwrap();
        };
        let answer = |key: Option<u8>, asks: Option<&[&str]>, version: &str| {
            let key = key.map(author);
            let sha = kept_signed(
                &lamp,
                "desk-wave",
                &says("desk_wave"),
                version,
                key.as_ref(),
                asks,
            );
            at_now(&lamp);
            ask(&lamp, &state, &format!("{{\"install_pushed\": \"{sha}\"}}"))
        };
        assert_eq!(answer(Some(1), None, "0.1.0")["code"], "asks_differ");
        assert_eq!(
            answer(Some(3), Some(&["speak"]), "0.1.0")["code"],
            "bad_package"
        );
        assert!(answer(Some(1), Some(&["speak"]), "0.1.0")["done"]["author"].is_string());
        assert_eq!(
            answer(Some(2), Some(&["speak"]), "0.2.0")["code"],
            "signer_changed"
        );
    }

    /// The owner's camera yes is keyed by name, computer, SHA-256 and
    /// author: a yes kept without the author is no yes for a shared skill.
    #[test]
    fn the_camera_yes_names_the_author_too() {
        use lamp_skills::package::{PICTURE_YES, picture_yes};
        let lamp = lamp();
        store_key(&lamp, &author(3));
        let toml = "tool = \"sees_desk\"\n[permissions]\ncamera = true\n[[do]]\nlook = true\n";
        let sha = kept_signed(
            &lamp,
            "sees-desk",
            toml,
            "0.1.0",
            Some(&author(1)),
            Some(&["camera"]),
        );
        assert!(install(&lamp, &sha).unwrap().held.is_some());
        let from = pushed_from(&lamp.folders.pushed.join("sees-desk")).unwrap();
        let yes = |author: Option<&str>| {
            fs::write(
                lamp.folders.pushed.join(PICTURE_YES),
                serde_json::json!([{
                    "name": "sees-desk", "client": "c_1", "sha256": sha, "author": author,
                }])
                .to_string(),
            )
            .unwrap();
            picture_yes(&lamp.folders.pushed)
        };
        assert!(yes(None).is_empty(), "a yes without the author");
        assert!(yes(Some("AAAA")).is_empty(), "another author's");
        assert!(yes(from.author.as_deref()).contains("sees-desk"));
    }

    #[test]
    fn a_push_refused_for_a_grant_says_no_grant_and_another_refusal_does_not() {
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let cases = [
            // Beyond the protocol's grants.
            (
                "tool = \"t\"\n[permissions]\nspeak = true\nmessages = true\n[[do]]\nsay = \"Say you will.\"\n".to_owned(),
                &["speak"][..],
                Some("no_grant"),
            ),
            // `hear`, as format 1's topic.
            (
                "[[trigger]]\ntopic = \"ears/classes\"\nfield = \"class\"\nequals = \"knock\"\n[ask]\npolicy = \"unprompted\"\n[permissions]\ntopics = [\"ears/classes\"]\nspeak = true\n[[do]]\nsay = \"Say hello.\"\n".to_owned(),
                &["speak"][..],
                Some("no_grant"),
            ),
            // A grant this computer was not given.
            (says("t"), &["light"][..], Some("no_grant")),
        ];
        for (toml, grants, code) in cases {
            let sha = kept(&lamp, "asks-more", &toml, "c_1", grants, now);
            let refused = ask(&lamp, &state, &format!("{{\"install_pushed\": \"{sha}\"}}"));
            assert!(refused["refused"].is_string(), "{refused}");
            assert_eq!(refused["code"].as_str(), code, "{refused}");
        }
        // Not a grant's: a name already on the lamp.
        skill(&lamp.folders.skills, "wiggle", &says("wiggle"));
        let sha = kept(&lamp, "wiggle", &says("other"), "c_1", &["speak"], now);
        let refused = ask(&lamp, &state, &format!("{{\"install_pushed\": \"{sha}\"}}"));
        assert!(refused["refused"].is_string(), "{refused}");
        assert!(refused.get("code").is_none(), "{refused}");
    }

    #[test]
    fn a_helper_given_no_developer_folder_refuses_every_pushed_request() {
        let lamp = lamp();
        let mut answer = Vec::new();
        lamp_skills::package::asked(
            &lamp.folders.skills,
            &lamp.folders.skills.join("no-made"),
            &lamp.folders.skills.join("no-state"),
            Duration::from_secs(2),
            None,
            None,
            &b"{\"list_pushed_by\": \"c_1\"}\n"[..],
            &mut answer,
        )
        .unwrap();
        let answer: serde_json::Value = serde_json::from_slice(&answer).unwrap();
        assert_eq!(answer["refused"], lamp_skills::package::PUSH_OFF);
    }

    /// The Reviewer-lead's P3 on #3879: the owner's yes to the picture is
    /// for that computer and those bytes, never the bare name.
    #[test]
    fn the_owners_yes_to_the_picture_is_for_that_push_alone() {
        let lamp = lamp();
        let state = lamp.work.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sees = "tool = \"sees_desk\"\n[permissions]\ncamera = true\n[[do]]\nlook = true\n";
        let sha = kept(&lamp, "sees-desk", sees, "c_1", &["body"], now);
        assert_eq!(
            ask(&lamp, &state, &format!("{{\"install_pushed\": \"{sha}\"}}"))["done"]["on"],
            false
        );
        assert!(lamp_skills::package::picture_yes(&lamp.folders.pushed).is_empty());
        // The owner's yes in the app.
        let on = ask_as(
            &lamp,
            &state,
            r#"{"on": "sees-desk"}"#,
            lamp_skills::package::Asker::App,
        );
        assert!(on.get("done").is_some(), "{on}");
        let yes = lamp_skills::package::picture_yes(&lamp.folders.pushed);
        assert!(yes.contains("sees-desk"));
        let listed = ask(&lamp, &state, r#"{"list_pushed_by": "c_1"}"#);
        assert_eq!(listed["done"][0]["on"], true, "{listed}");
        let (loaded, _) = load_all_with(&lamp.folders.skills, pushed(&lamp.folders, &yes));
        assert!(loaded.iter().any(|skill| skill.name == "sees-desk"));
        // A re-push of new bytes has no yes.
        let sha = kept(
            &lamp,
            "sees-desk",
            &format!("{sees}# again\n"),
            "c_1",
            &["body"],
            now,
        );
        ask(&lamp, &state, &format!("{{\"install_pushed\": \"{sha}\"}}"));
        assert!(lamp_skills::package::picture_yes(&lamp.folders.pushed).is_empty());
    }
}

/// The one definition of a push's `asks` (`package::grants_asked`), as a
/// table for the SDKs to mirror (blue and Ten on #3931): its names in the
/// protocol's order, body, light, speak, look, camera.
#[test]
fn the_asks_of_a_skill_are_grants_asked_in_the_protocols_order() {
    use lamp_skills::package::grants_asked;
    let rows: [(&str, &[&str]); 7] = [
        (
            "tool = \"t\"\n[permissions]\nspeak = true\n[[do]]\nsay = \"Say you will.\"\n",
            &["speak"],
        ),
        (
            "tool = \"t\"\n[permissions]\nbody = true\nlight = true\nspeak = true\n[[do]]\nbody = { mood = \"reading\", for = \"2m\" }\nlight = { colour = \"warm\" }\nsay = \"Say you will.\"\n",
            &["body", "light", "speak"],
        ),
        // Format 1's look is the picture: camera alone.
        (
            "tool = \"t\"\n[permissions]\nlook = true\n[[do]]\nlook = true\n",
            &["camera"],
        ),
        // Format 2's look is presence: look alone.
        (
            "format = 2\ntool = \"t\"\n[permissions]\nlook = true\nspeak = true\n[[do]]\nsay = \"Say you will.\"\n",
            &["speak", "look"],
        ),
        // Presence topics, format 1: look.
        (
            "[[trigger]]\ntopic = \"room/stood_up\"\nfield = \"stood_up\"\nequals = true\n[ask]\npolicy = \"unprompted\"\n[permissions]\ntopics = [\"room/stood_up\"]\nspeak = true\n[[do]]\nsay = \"Say hello.\"\n",
            &["speak", "look"],
        ),
        (
            "format = 2\ntool = \"t\"\n[permissions]\ncamera = true\n[[do]]\nlook = true\n",
            &["camera"],
        ),
        (
            "format = 2\ntool = \"t\"\n[permissions]\nlook = true\ncamera = true\n[[do]]\nlook = true\n",
            &["look", "camera"],
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (at, (toml, asks)) in rows.iter().enumerate() {
        let folder = skill(dir.path(), &format!("row-{at}"), toml);
        let loaded = lamp_skills::load(&folder).unwrap_or_else(|why| panic!("row {at}: {why}"));
        let names: Vec<&str> = grants_asked(&loaded)
            .iter()
            .map(|(grant, _)| *grant)
            .collect();
        assert_eq!(names, *asks, "row {at}");
    }
}
