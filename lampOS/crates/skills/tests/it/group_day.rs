//! A `[budget]` group's day, and its follow-ups (#6543, `docs/skills.md`,
//! "A budget shared by a group"): `per_day` caps the runs a group starts a
//! local day between its skills, counted and kept as `most_a_day` is, and
//! with `follows_count = true` a member's `follows` word is one of the
//! group's runs. A group naming neither is as before.
use lamp_skills::{Engine, holds, load};
use std::{
    fs,
    path::{Path, PathBuf},
};

const BOOT: &str = "boot-a";
/// 23:00 UTC on a day, the local time UTC.
const LATE_MS: u64 = 1_762_383_600_000;

/// A skill `name` with `toml` (its triggers and limits), lighting warm.
fn skill(dir: &Path, name: &str, toml: &str) -> lamp_skills::Skill {
    let folder = dir.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A test.\n---\n"),
    )
    .unwrap();
    fs::write(
        folder.join("skill.toml"),
        format!(
            "{toml}\n[ask]\npolicy = \"nudge\"\n[[do]]\nlight = {{ colour = \"warm\", for = \"5s\" }}\n\
             [permissions]\ntopics = [\"air/indoor\", \"air/outdoor\", \"voice/state\"]\nlight = true\n"
        ),
    )
    .unwrap();
    load(&folder).unwrap()
}

/// A nudge on stuffy `topic` air, once a spell, in the group `nudges`
/// with `budget` added to its `[budget]`.
fn member(dir: &Path, name: &str, topic: &str, budget: &str) -> lamp_skills::Skill {
    skill(
        dir,
        name,
        &format!(
            "[budget]\ngroup = \"nudges\"\nper_hour = 6\n{budget}\n\
             [trigger]\ntopic = \"{topic}\"\nfield = \"co2\"\nabove = 1000\n\
             rearm = {{ below = 1000, for = \"10m\" }}\n"
        ),
    )
}

/// Three of the group, the third naming `per_day = 2`.
fn three(dir: &Path) -> Vec<lamp_skills::Skill> {
    vec![
        member(dir, "a", "air/indoor", ""),
        member(dir, "b", "air/indoor", ""),
        member(dir, "c", "air/indoor", "per_day = 2"),
    ]
}

fn air(engine: &mut Engine, topic: &str, at_s: u64, co2: u32) -> Vec<String> {
    let runs = engine.observe(
        topic,
        at_s * 1_000_000,
        format!(r#"{{"co2":{co2}}}"#).as_bytes(),
        None,
    );
    engine.keep_holds(at_s * 1_000_000);
    runs.into_iter()
        .filter(|run| !run.calls.is_empty())
        .map(|run| run.skill)
        .collect()
}

fn stuffy(engine: &mut Engine, at_s: u64) -> Vec<String> {
    air(engine, "air/indoor", at_s, 1_200)
}

/// Good air from `at_s` for 11 minutes: every member's spell rearms.
fn rearm(engine: &mut Engine, at_s: u64) {
    air(engine, "air/indoor", at_s, 800);
    air(engine, "air/indoor", at_s + 660, 800);
}

fn clocked(skills: Vec<lamp_skills::Skill>) -> Engine {
    let mut engine = Engine::new(skills, true);
    engine.clock(0, 0, LATE_MS);
    engine
}

/// `per_day = 2`, named by one member, caps the group: two runs that day
/// between its three skills, whichever they are, with its hour still free.
/// Past the local midnight the group starts two again.
#[test]
fn a_groups_day_caps_its_runs_across_its_skills_and_begins_again_at_midnight() {
    let dir = tempfile::tempdir().unwrap();
    let skills = three(dir.path());
    assert_eq!(skills[2].budget_per_day, Some(2));
    assert_eq!(skills[0].budget_per_day, None);
    let mut engine = clocked(skills);
    assert_eq!(stuffy(&mut engine, 0), ["a", "b"]);
    rearm(&mut engine, 100);
    // 23:25: the day's two are run, its hour is not spent.
    assert!(stuffy(&mut engine, 1_500).is_empty());
    rearm(&mut engine, 1_600);
    // 00:01:40 the next day.
    assert_eq!(stuffy(&mut engine, 3_700), ["a", "b"]);
}

/// The group's day is kept beside the holds, as `most_a_day` is: after a
/// restart on the same boot the group stays at its most; another boot
/// takes it back as it takes back nothing.
#[test]
fn a_groups_day_stays_at_its_most_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let file: PathBuf = dir.path().join(holds::FILE);
    let start = |boot: &str, at_s: u64| {
        let mut engine = clocked(three(dir.path()));
        engine
            .keep_holds_in(file.clone(), boot.into(), at_s * 1_000_000)
            .unwrap();
        engine
    };
    let mut engine = start(BOOT, 0);
    assert_eq!(stuffy(&mut engine, 0), ["a", "b"]);
    let kept: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(kept["limits"]["groups"]["nudges"]["day"][1], 2, "{kept}");
    drop(engine);
    let mut engine = start(BOOT, 20);
    assert!(stuffy(&mut engine, 20).is_empty());
    drop(engine);
    let mut engine = start("boot-b", 30);
    assert_eq!(stuffy(&mut engine, 30), ["a", "b"]);
}

/// A skill whose firmer word `follows` its first on a resting voice, and a
/// sip on the outdoor air, both of `nudges` at `per_hour`, `budget` added
/// to the first's `[budget]`.
fn with_word(dir: &Path, per_hour: u32, budget: &str) -> Vec<lamp_skills::Skill> {
    vec![
        skill(
            dir,
            "eyes",
            &format!(
                "[budget]\ngroup = \"nudges\"\nper_hour = {per_hour}\n{budget}\n\
                 [[trigger]]\nname = \"first\"\ntopic = \"air/indoor\"\nfield = \"co2\"\nabove = 1000\nrearm = \"ends\"\n\
                 [[trigger]]\nname = \"builds\"\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n\
                 rearm = \"ends\"\nfollows = {{ trigger = \"first\", within = \"10m\" }}\n"
            ),
        ),
        skill(
            dir,
            "sip",
            &format!(
                "[budget]\ngroup = \"nudges\"\nper_hour = {per_hour}\n\
                 [trigger]\ntopic = \"air/outdoor\"\nfield = \"co2\"\nabove = 1000\nrearm = \"ends\"\n"
            ),
        ),
    ]
}

fn resting(engine: &mut Engine, at_s: u64) -> Vec<String> {
    engine
        .observe(
            "voice/state",
            at_s * 1_000_000,
            br#"{"state":"resting"}"#,
            None,
        )
        .into_iter()
        .filter(|run| !run.calls.is_empty())
        .map(|run| run.skill)
        .collect()
}

/// With no `follows_count` the word after a run is that run's, as before:
/// never held back by the group's spent hour, and it spends none of it.
/// With `follows_count = true` it is one of the group's runs: held back by
/// a spent hour, a day or a gap, and spending them.
#[test]
fn a_follow_up_counts_toward_its_group_only_when_the_group_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let skills = with_word(dir.path(), 1, "");
    assert!(!skills[0].budget_follows_count);
    // Before: the first spends the hour, the word still comes.
    let mut engine = Engine::new(skills, true);
    assert_eq!(stuffy(&mut engine, 0), ["eyes"]);
    assert_eq!(resting(&mut engine, 1), ["eyes"]);
    // Counted: the hour the first spent holds the word back.
    let mut engine = Engine::new(with_word(dir.path(), 1, "follows_count = true"), true);
    assert_eq!(stuffy(&mut engine, 0), ["eyes"]);
    assert!(resting(&mut engine, 1).is_empty());

    // Two an hour: the word spends the second, so the sip waits; not
    // counted, the sip has it.
    let mut engine = Engine::new(with_word(dir.path(), 2, ""), true);
    assert_eq!(stuffy(&mut engine, 0), ["eyes"]);
    assert_eq!(resting(&mut engine, 1), ["eyes"]);
    assert_eq!(air(&mut engine, "air/outdoor", 2, 1_200), ["sip"]);
    let mut engine = Engine::new(with_word(dir.path(), 2, "follows_count = true"), true);
    assert_eq!(stuffy(&mut engine, 0), ["eyes"]);
    assert_eq!(resting(&mut engine, 1), ["eyes"]);
    assert!(air(&mut engine, "air/outdoor", 2, 1_200).is_empty());

    // A day of two, with the word counted: the first and the word are it.
    let mut engine = clocked(with_word(
        dir.path(),
        6,
        "per_day = 2\nfollows_count = true",
    ));
    assert_eq!(stuffy(&mut engine, 0), ["eyes"]);
    assert_eq!(resting(&mut engine, 1), ["eyes"]);
    assert!(air(&mut engine, "air/outdoor", 2, 1_200).is_empty());
    // A gap, with the word counted, holds it back too.
    let mut engine = Engine::new(
        with_word(dir.path(), 6, "gap = \"5m\"\nfollows_count = true"),
        true,
    );
    assert_eq!(stuffy(&mut engine, 0), ["eyes"]);
    assert!(resting(&mut engine, 1).is_empty());
}

/// `per_day` is 1 to 48; a group naming neither key loads as before.
#[test]
fn per_day_is_bounded_and_both_keys_are_off_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let plain = member(dir.path(), "plain", "air/indoor", "");
    assert_eq!(
        (plain.budget_per_day, plain.budget_follows_count),
        (None, false)
    );
    for bad in ["per_day = 0", "per_day = 49"] {
        let folder = dir.path().join("bad");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("SKILL.md"),
            "---\nname: bad\ndescription: A test.\n---\n",
        )
        .unwrap();
        fs::write(
            folder.join("skill.toml"),
            format!(
                "[budget]\ngroup = \"nudges\"\nper_hour = 1\n{bad}\n\
                 [trigger]\ntopic = \"air/indoor\"\nfield = \"co2\"\nabove = 1000\n\
                 [ask]\npolicy = \"nudge\"\n[[do]]\nlight = {{ dim = 1 }}\n\
                 [permissions]\ntopics = [\"air/indoor\"]\nlight = true\n"
            ),
        )
        .unwrap();
        let err = load(&folder).unwrap_err().to_string();
        assert!(err.contains("per_day is 1 to 48"), "{bad}: {err}");
    }
}
