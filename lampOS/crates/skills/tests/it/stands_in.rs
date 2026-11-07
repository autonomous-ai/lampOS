//! Quiet focus and Rest your eyes in one engine (#5620, the PM's call (a)):
//! both are due when someone looks up after 45 minutes head down, and on a
//! lamp that installed Quiet focus its word stands in for Rest your eyes'.
//! A lamp without it, or a sitting with no focus, is as before. Replayed
//! from Quiet focus's own case, through the real loader and engine.
use lamp_skills::{
    Call, Engine, Pushed, Recorded, Skill, load_each, load_each_with, load_with, read_recordings,
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

/// `stands_in_for` is a `follows` word's, and names another skill.
#[test]
fn stands_in_for_goes_with_follows_and_names_another_skill() {
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
    let own = toml.replace(
        "stands_in_for = \"rest-your-eyes\"",
        "stands_in_for = \"quiet-focus\"",
    );
    assert!(with(&own).unwrap_err().contains("is this skill"));
    // On `focused`, which follows nothing.
    let unfollowed = toml
        .replace("stands_in_for = \"rest-your-eyes\"\n", "")
        .replace(
            "rearm = \"leaves\"          # once each time they settle in",
            "rearm = \"leaves\"\nstands_in_for = \"rest-your-eyes\"",
        );
    assert!(
        with(&unfollowed)
            .unwrap_err()
            .contains("stands_in_for goes with follows")
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

/// The reviewer's decoy (P1 on #5704): Quiet focus's `looked_up` naming
/// `names`, in its own `[budget]` group or `group`.
fn decoy(root: &Path, names: &str, group: &str) -> PathBuf {
    copy(
        root,
        "quiet-focus",
        &[
            (
                "stands_in_for = \"rest-your-eyes\"",
                &format!("stands_in_for = \"{names}\""),
            ),
            ("group = \"wellbeing\"", &format!("group = \"{group}\"")),
        ],
    )
}

/// Whether Rest your eyes (or its copy) spoke in the sitting.
fn rest_spoke(said: &[(String, u64)]) -> bool {
    said.iter().any(|(skill, _)| skill == "rest-your-eyes")
}

/// A decoy in another `[budget]` group holds nothing back: Rest your eyes
/// still speaks when they look up, and on a lamp with both the decoy is
/// refused at load.
#[test]
fn a_word_in_another_group_holds_nothing_back() {
    let dir = tempfile::tempdir().unwrap();
    let word = decoy(dir.path(), "rest-your-eyes", "focus");
    let said = says_of(
        vec![loaded(&word), loaded(&skills_root().join("rest-your-eyes"))],
        &case(),
    );
    assert!(rest_spoke(&said), "{said:?}");
    copy(dir.path(), "rest-your-eyes", &[]);
    let decoy_loaded = load_each(dir.path())
        .into_iter()
        .find(|(folder, _)| folder.ends_with("quiet-focus"))
        .unwrap()
        .1;
    assert!(
        decoy_loaded
            .as_ref()
            .is_err_and(|why| why.contains("its own [budget] group")),
        "{:?}",
        decoy_loaded.map(|skill| skill.name)
    );
}

/// A decoy naming a `warning` skill holds nothing back, even in its own
/// group: a warning (Smoky air's policy) is never kept for another's word,
/// and on a lamp with both the decoy is refused at load.
#[test]
fn a_word_for_a_warning_holds_nothing_back() {
    let dir = tempfile::tempdir().unwrap();
    let word = decoy(dir.path(), "rest-your-eyes", "wellbeing");
    let warning = copy(
        dir.path(),
        "rest-your-eyes",
        &[("policy = \"nudge\"", "policy = \"warning\"")],
    );
    let said = says_of(vec![loaded(&word), loaded(&warning)], &case());
    assert!(rest_spoke(&said), "{said:?}");
    let decoy_loaded = load_each(dir.path())
        .into_iter()
        .find(|(folder, _)| folder.ends_with("quiet-focus"))
        .unwrap()
        .1;
    assert!(
        decoy_loaded
            .as_ref()
            .is_err_and(|why| why.contains("a warning")),
        "{:?}",
        decoy_loaded.map(|skill| skill.name)
    );
    // Smoky air itself, beside a decoy naming it.
    let smoky = tempfile::tempdir().unwrap();
    decoy(smoky.path(), "smoky-air", "wellbeing");
    copy(smoky.path(), "smoky-air", &[]);
    let decoy_loaded = load_each(smoky.path())
        .into_iter()
        .find(|(folder, _)| folder.ends_with("quiet-focus"))
        .unwrap()
        .1;
    assert!(decoy_loaded.is_err(), "a decoy naming smoky-air loaded");
}

/// A decoy naming a skill that isn't loaded does nothing: Rest your eyes
/// speaks as without it, and the decoy loads (the skill it names may be
/// installed later, and is then held to its group and policy).
#[test]
fn a_word_for_a_skill_not_loaded_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let word = decoy(dir.path(), "no-such-skill", "wellbeing");
    let said = says_of(
        vec![loaded(&word), loaded(&skills_root().join("rest-your-eyes"))],
        &case(),
    );
    assert!(rest_spoke(&said), "{said:?}");
}

/// A pushed skill may not use `stands_in_for`: a computer's skill never
/// holds back a lamp skill's chance.
#[test]
fn a_pushed_skill_with_stands_in_for_is_refused() {
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
    assert_eq!(result(""), Ok("desk-decoy".to_owned()));
    let refused = result("stands_in_for = \"rest-your-eyes\"\n");
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("stands_in_for")),
        "{refused:?}"
    );
}
