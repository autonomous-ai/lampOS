//! The carry of a stop from Sit back to Posture (#6543, the Reviewer-lead's
//! P2 on #8377, `docs/skills.md`): until Posture was its own skill,
//! "posture" and "slouch" were Sit back's stop words, so an off, a snooze or
//! a `[stop.done]` rest of `sit-back` from before this build is carried to
//! `posture` once, at the first load ([`Engine::keep_off_in`]), and never
//! again ([`lamp_skills::CARRIED_FILE`]). A restart is a new [`Engine`]
//! reading the files the old one wrote, as the program's start does; a
//! build from before is one that never called `keep_off_in` with the carry.
use lamp_skills::{CARRIED_FILE, Engine, OFF_FILE, holds, load, read_names};
use std::{fs, path::Path};

const BOOT: &str = "boot-a";

/// A nudge on stuffy air named `name`, with its own stop `stop_<name>`
/// (`more` added to the `[stop]`).
fn skill(dir: &Path, name: &str, more: &str) -> lamp_skills::Skill {
    let folder = dir.join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: A test.\n---\n"),
    )
    .unwrap();
    let tool = name.replace('-', "_");
    fs::write(
        folder.join("skill.toml"),
        format!(
            "[trigger]\ntopic = \"air/indoor\"\nfield = \"co2\"\nabove = 1000\nfor = \"50m\"\ncooldown = \"50m\"\n\
             [stop]\ntool = \"stop_{tool}\"\nabout = \"When someone asks to stop the {name} nudge.\"\n{more}\n\
             [ask]\npolicy = \"nudge\"\n[[do]]\nlight = {{ colour = \"warm\", for = \"5s\" }}\n\
             [permissions]\ntopics = [\"air/indoor\"]\nlight = true\n"
        ),
    )
    .unwrap();
    load(&folder).unwrap()
}

const DONE: &str = "[stop.done]\nhow = \"sat_back\"\nfor = \"10m\"\nabout = \"they just sat back\"";

fn skills(dir: &Path) -> Vec<lamp_skills::Skill> {
    vec![skill(dir, "sit-back", DONE), skill(dir, "posture", "")]
}

/// A program started at `at_s`, its off file and carries in `state`, its
/// holds in `runtime`, as the program's start reads them.
fn started(dir: &Path, state: &Path, runtime: &Path, at_s: u64) -> Engine {
    let mut engine = Engine::new(skills(dir), true);
    engine.keep_off_in(state).unwrap();
    engine
        .keep_holds_in(runtime.join(holds::FILE), BOOT.into(), at_s * 1_000_000)
        .unwrap();
    engine
}

/// A build from before the carry: its holds kept, its off file written by
/// `sit-back`'s stop called with `how`, at 100 s.
fn before(dir: &Path, state: &Path, runtime: &Path, how: &str) {
    let mut engine = Engine::new(skills(dir), true);
    engine
        .keep_holds_in(runtime.join(holds::FILE), BOOT.into(), 0)
        .unwrap();
    if how == "off" {
        fs::write(state.join(OFF_FILE), "sit-back\n").unwrap();
    } else {
        engine
            .tool(
                "stop_sit_back",
                &serde_json::json!({ "how": how }),
                100_000_000,
            )
            .unwrap();
    }
    engine.keep_holds(110_000_000);
}

fn carried(state: &Path) -> Vec<String> {
    read_names(&state.join(CARRIED_FILE))
        .unwrap()
        .into_iter()
        .collect()
}

#[test]
fn an_off_of_sit_back_from_before_turns_posture_off_once() {
    let (dir, state, runtime) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    before(dir.path(), state.path(), runtime.path(), "off");
    let engine = started(dir.path(), state.path(), runtime.path(), 120);
    assert!(engine.stopped("posture", 120_000_000), "posture off");
    assert!(
        engine.stopped("sit-back", 120_000_000),
        "sit-back still off"
    );
    let off = read_names(&state.path().join(OFF_FILE)).unwrap();
    assert!(
        off.contains("posture") && off.contains("sit-back"),
        "{off:?}"
    );
    assert_eq!(carried(state.path()), ["posture"]);
}

#[test]
fn a_snooze_of_sit_back_from_before_snoozes_posture_as_long() {
    let (dir, state, runtime) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    before(dir.path(), state.path(), runtime.path(), "snooze");
    let engine = started(dir.path(), state.path(), runtime.path(), 120);
    assert!(engine.stopped("sit-back", 120_000_000));
    assert!(engine.stopped("posture", 120_000_000), "posture snoozed");
    let off = read_names(&state.path().join(OFF_FILE)).unwrap();
    assert!(!off.contains("posture"), "a snooze is not an off: {off:?}");
    // The two end together: the next morning, at most 24 h on.
    let after = 120_000_000 + 24 * 3_600_000_000;
    assert!(!engine.stopped("posture", after) && !engine.stopped("sit-back", after));
    assert_eq!(carried(state.path()), ["posture"]);
}

#[test]
fn a_done_rest_of_sit_back_from_before_rests_posture_as_long() {
    let (dir, state, runtime) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    before(dir.path(), state.path(), runtime.path(), "sat_back");
    let mut engine = started(dir.path(), state.path(), runtime.path(), 120);
    // Rested 10 minutes from 100 s: until 700 s, as sit-back.
    assert!(engine.stopped("posture", 699_000_000), "posture resting");
    assert!(!engine.stopped("posture", 700_000_000), "and no longer");
    // Kept as a rest, not a snooze alone: its done_until is written.
    engine.keep_holds(130_000_000);
    let kept: serde_json::Value =
        serde_json::from_slice(&fs::read(runtime.path().join(holds::FILE)).unwrap()).unwrap();
    let skills = &kept["limits"]["skills"];
    assert_eq!(skills["posture"]["done_until_us"], 700_000_000u64, "{kept}");
    assert_eq!(
        skills["posture"]["done_until_us"],
        skills["sit-back"]["done_until_us"]
    );
}

#[test]
fn posture_turned_on_again_is_not_carried_off_again() {
    let (dir, state, runtime) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    before(dir.path(), state.path(), runtime.path(), "off");
    let mut engine = started(dir.path(), state.path(), runtime.path(), 120);
    assert!(engine.stopped("posture", 120_000_000));
    engine
        .tool(
            "stop_posture",
            &serde_json::json!({ "how": "on" }),
            130_000_000,
        )
        .unwrap();
    engine.keep_holds(130_000_000);
    drop(engine);
    let engine = started(dir.path(), state.path(), runtime.path(), 140);
    assert!(!engine.stopped("posture", 140_000_000), "posture stays on");
    assert!(
        engine.stopped("sit-back", 140_000_000),
        "sit-back stays off"
    );
}

#[test]
fn nothing_is_carried_when_sit_back_was_not_stopped() {
    let (dir, state, runtime) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let mut engine = started(dir.path(), state.path(), runtime.path(), 120);
    assert!(!engine.stopped("posture", 120_000_000));
    assert_eq!(
        carried(state.path()),
        ["posture"],
        "made, with nothing to carry"
    );
    // Sit back stopped after this build is Sit back's alone, then and after
    // a restart.
    engine
        .tool(
            "stop_sit_back",
            &serde_json::json!({ "how": "off" }),
            130_000_000,
        )
        .unwrap();
    engine
        .tool(
            "stop_sit_back",
            &serde_json::json!({ "how": "snooze" }),
            131_000_000,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    engine.keep_holds(131_000_000);
    assert!(!engine.stopped("posture", 131_000_000));
    drop(engine);
    let engine = started(dir.path(), state.path(), runtime.path(), 140);
    assert!(engine.stopped("sit-back", 140_000_000));
    assert!(
        !engine.stopped("posture", 140_000_000),
        "posture not stopped"
    );
}

#[test]
fn an_off_not_kept_leaves_the_carry_to_the_next_load() {
    let (dir, state, runtime) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    before(dir.path(), state.path(), runtime.path(), "off");
    // A link where the off file's new copy is made: writing the off fails,
    // while the carries file beside it can still be written.
    let new = state.path().join(OFF_FILE).with_extension("new");
    std::os::unix::fs::symlink(state.path().join("elsewhere"), &new).unwrap();
    let engine = started(dir.path(), state.path(), runtime.path(), 120);
    assert!(
        engine.stopped("posture", 120_000_000),
        "posture off for now"
    );
    let off = read_names(&state.path().join(OFF_FILE)).unwrap();
    assert!(!off.contains("posture"), "the off was not kept: {off:?}");
    assert!(
        carried(state.path()).is_empty(),
        "a carry not kept is not marked made"
    );
    drop(engine);
    fs::remove_file(&new).unwrap();
    let engine = started(dir.path(), state.path(), runtime.path(), 130);
    assert!(engine.stopped("posture", 130_000_000), "posture off again");
    let off = read_names(&state.path().join(OFF_FILE)).unwrap();
    assert!(off.contains("posture"), "{off:?}");
    assert_eq!(carried(state.path()), ["posture"]);
}
