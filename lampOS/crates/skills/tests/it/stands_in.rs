//! Quiet focus and the wellbeing nudges in one engine (#5620, the PM's call
//! (a)): Rest your eyes, and with `LAMP_NUDGES_PRESENT` Sip of water too,
//! are due when someone looks up after 45 minutes head down, and on a lamp
//! that installed Quiet focus its word stands in for its whole `[budget]`
//! group (`stands_in_for_group`). A lamp without it, or a sitting with no
//! focus, is as before. Replayed from Quiet focus's own case, through the
//! real loader and engine.
use lamp_skills::{
    Call, Engine, Pushed, Recorded, Skill, load_each_with, load_with, read_recordings,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 1 January 2026, 10:00 UTC: the case's `clock = "10:00"`, as
/// `lamp-skills test` replays it.
const TEN_O_CLOCK_MS: u64 = 1_767_225_600_000 + 10 * 3_600_000;

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

fn case() -> Vec<Recorded> {
    read_recordings(
        &skills_root()
            .join("quiet-focus")
            .join("tests")
            .join("a-word-when-they-look-up"),
    )
    .unwrap()
}

/// Each say made, by skill and the second it was made from the case's
/// first message, with `skills` loaded (each turned on, as a case is).
fn says(skills: &[&str], messages: &[Recorded]) -> Vec<(String, u64)> {
    let skills = skills
        .iter()
        .map(|name| loaded(&skills_root().join(name)))
        .collect();
    says_of(skills, messages)
}

/// The skill in `folder`, turned on, as a case loads it.
fn loaded(folder: &Path) -> Skill {
    let mut skill = load_with(folder, false).unwrap();
    skill.enabled = true;
    skill
}

/// [`says`], with these skills loaded.
fn says_of(skills: Vec<Skill>, messages: &[Recorded]) -> Vec<(String, u64)> {
    runs_of(skills, messages)
        .into_iter()
        .filter(|(run, _)| run.calls.iter().any(|call| matches!(call, Call::Say(_))))
        .map(|(run, at_s)| (run.skill, at_s))
        .collect()
}

/// Each run made, with the second it was made from the case's first
/// message, with these skills loaded.
fn runs_of(skills: Vec<Skill>, messages: &[Recorded]) -> Vec<(lamp_skills::Run, u64)> {
    let mut engine = Engine::new(skills, true);
    let first_us = messages.first().map_or(0, |message| message.header.at_us);
    engine.clock(0, first_us, TEN_O_CLOCK_MS);
    let mut made = Vec::new();
    for message in messages {
        let at_us = message.header.at_us;
        let at_s = (at_us - first_us) / 1_000_000;
        engine.clock(0, at_us, TEN_O_CLOCK_MS + (at_us - first_us) / 1_000);
        made.extend(engine.tick(at_us).into_iter().map(|run| (run, at_s)));
        made.extend(
            engine
                .observe(
                    &message.header.topic,
                    at_us,
                    &message.payload,
                    message.header.expires_ms,
                )
                .into_iter()
                .map(|run| (run, at_s)),
        );
    }
    made
}

/// The same case with the eyes saying `at_desk` all along: never head down,
/// so no focus comes due.
fn never_head_down() -> Vec<Recorded> {
    case()
        .into_iter()
        .map(|mut message| {
            if message.header.topic == "room/activity" {
                let mut value: Value = serde_json::from_slice(&message.payload).unwrap();
                if value["state"] == "head_down" {
                    value["state"] = "at_desk".into();
                }
                message.payload = serde_json::to_vec(&value).unwrap();
            }
            message
        })
        .collect()
}

/// Head down from 3 minutes, the focus due at 48, a look up at 50: with both
/// installed, Quiet focus's word then, and no Rest your eyes line in the
/// sitting (lamp-9adf, 7 Nov: Rest your eyes spoke 5 s into the look up and
/// took the wellbeing group's gap, so the word was refused).
#[test]
fn quiet_focus_word_stands_in_for_rest_your_eyes() {
    let said = says(&["quiet-focus", "rest-your-eyes"], &case());
    assert_eq!(
        said.iter()
            .map(|(skill, _)| skill.as_str())
            .collect::<Vec<_>>(),
        ["quiet-focus"],
        "{said:?}"
    );
    assert!((50 * 60..51 * 60).contains(&said[0].1), "{said:?}");
}

/// Each skill under `skills/` of the wellbeing `[budget]` group, turned on,
/// with its `[present]` applied as the product's `LAMP_NUDGES_PRESENT=on`
/// does.
fn wellbeing_present() -> Vec<Skill> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(skills_root())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|folder| {
            std::fs::read_to_string(folder.join("skill.toml"))
                .is_ok_and(|toml| toml.contains("\n[budget]\ngroup = \"wellbeing\""))
        })
        .collect();
    folders.sort();
    folders
        .iter()
        .map(|folder| {
            let mut skill = load_with(folder, true).unwrap();
            skill.enabled = true;
            skill
        })
        .collect()
}

/// The replay on lamp-9adf (7 Nov) with the product's `[present]`: Sip of
/// water, of the wellbeing group too, came due at the same look up, spoke
/// first and spent the group's hour, so the word was refused. Every
/// wellbeing skill is held while the word is to come: only Quiet focus
/// speaks at the look up.
#[test]
fn with_every_wellbeing_nudge_present_only_the_word_speaks_at_the_look_up() {
    let skills = wellbeing_present();
    let names: Vec<_> = skills.iter().map(|skill| skill.name.as_str()).collect();
    for name in ["quiet-focus", "rest-your-eyes", "sip-of-water"] {
        assert!(names.contains(&name), "{names:?}");
    }
    let said = says_of(skills, &case());
    let at_look_up: Vec<_> = said
        .iter()
        .filter(|(_, at_s)| (50 * 60..51 * 60).contains(at_s))
        .map(|(skill, _)| skill.as_str())
        .collect();
    assert_eq!(at_look_up, ["quiet-focus"], "{said:?}");
    assert!(
        said.iter()
            .all(|(_, at_s)| *at_s >= 50 * 60 || *at_s < 48 * 60),
        "a wellbeing nudge spoke while the word was to come: {said:?}"
    );
}

/// The reviewer's bound on #5704: a word that never comes does not hold the
/// group for good. A Quiet focus copy whose focus comes due again and again
/// (each 5 minutes with a face there) and whose word never holds: the first
/// window closes without the word, so the group is not held again until the
/// word has run, and Rest your eyes speaks when they look up, as without it.
#[test]
fn a_word_that_never_comes_holds_the_group_once_at_most() {
    let dir = tempfile::tempdir().unwrap();
    let toml =
        std::fs::read_to_string(skills_root().join("quiet-focus").join("skill.toml")).unwrap();
    let start = toml.find("[[trigger]]\nname = \"focused\"").unwrap();
    let end = start + toml[start..].find("\n\n").unwrap();
    let again = "[[trigger]]\nname = \"focused\"\ntopic = \"humans/faces/tracked\"\n\
                 when = \"present\"\nfor = \"1m\"\ngap = \"2m\"\ncooldown = \"5m\"";
    let never = copy(
        dir.path(),
        "quiet-focus",
        &[
            (&toml[start..end], again),
            ("equals = \"at_desk\"", "equals = \"never\""),
        ],
    );
    let runs = runs_of(
        vec![
            loaded(&never),
            loaded(&skills_root().join("rest-your-eyes")),
        ],
        &case(),
    );
    let said: Vec<_> = runs
        .iter()
        .filter(|(run, _)| run.calls.iter().any(|call| matches!(call, Call::Say(_))))
        .map(|(run, at_s)| (run.skill.clone(), *at_s))
        .collect();
    assert!(rest_spoke(&said), "{said:?}");
    assert!(
        said.iter()
            .all(|(skill, at_s)| skill != "rest-your-eyes" || (50 * 60..51 * 60).contains(at_s)),
        "{said:?}"
    );
}

/// A product lamp, without Quiet focus: Rest your eyes says its line when
/// they look up, as today.
#[test]
fn rest_your_eyes_alone_still_speaks_in_that_sitting() {
    let said = says(&["rest-your-eyes"], &case());
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].0, "rest-your-eyes");
    assert!((50 * 60..51 * 60).contains(&said[0].1), "{said:?}");
}

/// Quiet focus installed, but never head down: no focus is pending, and
/// Rest your eyes says its line at 45 minutes, as it would without it.
#[test]
fn with_quiet_focus_and_no_focus_rest_your_eyes_still_speaks() {
    let said = says(&["quiet-focus", "rest-your-eyes"], &never_head_down());
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].0, "rest-your-eyes");
    assert!((45 * 60..46 * 60).contains(&said[0].1), "{said:?}");
}

/// The reviewer's P3 on #5704: the wait line is said only once the held
/// skill's `[[while]]` holds. A Rest your eyes whose `[[while]]` never holds
/// would not have spoken anyway, so the journal never says it waits for
/// Quiet focus's word, and the word is still said.
#[test]
fn no_wait_line_while_the_held_skills_while_does_not_hold() {
    let dir = tempfile::tempdir().unwrap();
    let never = copy(
        dir.path(),
        "rest-your-eyes",
        &[(
            "[active]",
            "[[while]]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"never\"\n\n[active]",
        )],
    );
    let runs = runs_of(
        vec![loaded(&skills_root().join("quiet-focus")), loaded(&never)],
        &case(),
    );
    let waits: Vec<_> = runs
        .iter()
        .flat_map(|(run, at_s)| run.events.iter().map(move |event| (event, at_s)))
        .filter(|(event, _)| event.why.starts_with("waits:"))
        .map(|(event, at_s)| (event.skill.clone(), event.why.clone(), *at_s))
        .collect();
    assert!(waits.is_empty(), "{waits:?}");
    let said: Vec<_> = runs
        .iter()
        .filter(|(run, _)| run.calls.iter().any(|call| matches!(call, Call::Say(_))))
        .map(|(run, _)| run.skill.as_str())
        .collect();
    assert_eq!(said, ["quiet-focus"]);
}

/// `stands_in_for_group` is a `follows` word's, in a skill with a
/// `[budget]` group.
#[test]
fn stands_in_for_group_goes_with_follows_and_a_budget_group() {
    let dir = tempfile::tempdir().unwrap();
    let from = skills_root().join("quiet-focus");
    let toml = std::fs::read_to_string(from.join("skill.toml")).unwrap();
    let with = |text: &str| {
        let folder = dir.path().join("quiet-focus");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::copy(from.join("SKILL.md"), folder.join("SKILL.md")).unwrap();
        std::fs::write(folder.join("skill.toml"), text).unwrap();
        lamp_skills::load(&folder)
    };
    assert!(with(&toml).is_ok());
    assert!(!toml.contains("stands_in_for ="), "the old key");
    let no_group = toml[..toml.find("[present.budget]").unwrap()]
        .replace("[budget]\ngroup = \"wellbeing\"\nper_hour = 1\n", "");
    assert!(
        with(&no_group)
            .unwrap_err()
            .contains("stands_in_for_group needs the skill's own [budget] group")
    );
    let off = toml.replace("stands_in_for_group = true", "stands_in_for_group = false");
    assert!(with(&off).is_ok());
    // On `focused`, which follows nothing.
    let unfollowed = toml.replace("stands_in_for_group = true\n", "").replace(
        "rearm = \"leaves\"          # once each time they settle in",
        "rearm = \"leaves\"\nstands_in_for_group = true",
    );
    assert!(
        with(&unfollowed)
            .unwrap_err()
            .contains("stands_in_for_group goes with follows")
    );
}

/// A copy of the skill `name` in `root`, its `skill.toml` with each
/// `(from, to)` replaced (each must be there).
fn copy(root: &Path, name: &str, edits: &[(&str, &str)]) -> PathBuf {
    let from = skills_root().join(name);
    let folder = root.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::copy(from.join("SKILL.md"), folder.join("SKILL.md")).unwrap();
    let mut toml = std::fs::read_to_string(from.join("skill.toml")).unwrap();
    for (was, now) in edits {
        assert!(toml.contains(was), "{name}: {was:?}");
        toml = toml.replace(was, now);
    }
    std::fs::write(folder.join("skill.toml"), toml).unwrap();
    folder
}

/// The reviewer's decoy (P1 on #5704): Quiet focus in the `[budget]` group
/// `group`.
fn decoy(root: &Path, group: &str) -> PathBuf {
    copy(
        root,
        "quiet-focus",
        &[("group = \"wellbeing\"", &format!("group = \"{group}\""))],
    )
}

/// Whether Rest your eyes (or its copy) spoke in the sitting.
fn rest_spoke(said: &[(String, u64)]) -> bool {
    said.iter().any(|(skill, _)| skill == "rest-your-eyes")
}

/// A word in another `[budget]` group holds nothing back: Rest your eyes
/// still speaks when they look up.
#[test]
fn a_word_in_another_group_holds_nothing_back() {
    let dir = tempfile::tempdir().unwrap();
    let word = decoy(dir.path(), "focus");
    let said = says_of(
        vec![loaded(&word), loaded(&skills_root().join("rest-your-eyes"))],
        &case(),
    );
    assert!(rest_spoke(&said), "{said:?}");
}

/// A `warning` skill of the word's own group is never held: a warning
/// (Smoky air's policy) never waits for another's word.
#[test]
fn a_word_holds_no_warning_of_its_group() {
    let dir = tempfile::tempdir().unwrap();
    let warning = copy(
        dir.path(),
        "rest-your-eyes",
        &[("policy = \"nudge\"", "policy = \"warning\"")],
    );
    let said = says_of(
        vec![loaded(&skills_root().join("quiet-focus")), loaded(&warning)],
        &case(),
    );
    assert!(rest_spoke(&said), "{said:?}");
}

/// A pushed skill may not use `stands_in_for_group`: a computer's skill
/// never holds back a lamp skill's chance.
#[test]
fn a_pushed_skill_with_stands_in_for_group_is_refused() {
    let toml = |key: &str| {
        format!(
            "kind = \"installable\"\n\
             [permissions]\nspeak = true\ntopics = [\"humans/faces/tracked\"]\n\
             [[trigger]]\nname = \"there\"\ntopic = \"humans/faces/tracked\"\nwhen = \"present\"\nfor = \"1m\"\n\
             [[trigger]]\nname = \"word\"\ntopic = \"humans/faces/tracked\"\nwhen = \"present\"\nfor = \"2m\"\nrearm = \"leaves\"\n\
             follows = {{ trigger = \"there\", within = \"10m\" }}\n{key}\
             [ask]\npolicy = \"nudge\"\n\
             [[do]]\non = \"word\"\nsay = \"Say hello.\"\n"
        )
    };
    let result = |key: &str| {
        let dir = tempfile::tempdir().unwrap();
        let (root, dev) = (dir.path().join("skills"), dir.path().join("skills-dev"));
        std::fs::create_dir_all(&root).unwrap();
        let folder = dev.join("desk-decoy");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: desk-decoy\ndescription: A test.\n---\n",
        )
        .unwrap();
        std::fs::write(folder.join("skill.toml"), toml(key)).unwrap();
        let yes = BTreeSet::new();
        load_each_with(
            &root,
            Some(Pushed {
                dir: &dev,
                picture_yes: &yes,
            }),
        )
        .pop()
        .unwrap()
        .1
        .map(|skill| skill.name)
    };
    let group = "[budget]\ngroup = \"wellbeing\"\nper_hour = 1\n";
    assert_eq!(result(group), Ok("desk-decoy".to_owned()));
    let refused = result(&format!("stands_in_for_group = true\n{group}"));
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("a pushed skill may not use stands_in_for_group")),
        "{refused:?}"
    );
}
