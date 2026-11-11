//! A long held trigger kept across a short restart of lamp-skills
//! (`src/holds.rs`, `docs/skills.md`, "A hold kept across a restart"): each
//! update restarts the program, and a 50-minute sitting began again at
//! every one. A restart is a new [`Engine`] reading the file the old one
//! wrote, as the program's start does.
use lamp_skills::{Engine, holds, load};
use std::{
    fs,
    path::{Path, PathBuf},
};

const BOOT: &str = "boot-a";

/// A nudge after 50 minutes of stuffy air, a 2-minute gap, and again 50
/// minutes after its last run while it holds: the present water nudge's
/// shape, on a topic a test can say. `more` is added to the trigger, or,
/// as `COOLDOWN|more`, its cooldown is that.
fn skill(dir: &Path, more: &str) -> lamp_skills::Skill {
    let (cooldown, more) = more.split_once('|').unwrap_or(("50m", more));
    let folder = dir.join("sip");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        "---\nname: sip\ndescription: A test.\n---\n",
    )
    .unwrap();
    fs::write(
        folder.join("skill.toml"),
        format!(
            "[trigger]\ntopic = \"air/indoor\"\nfield = \"co2\"\nabove = 1000\nfor = \"50m\"\ngap = \"2m\"\ncooldown = \"{cooldown}\"\n{more}\n\
             [ask]\npolicy = \"nudge\"\n[[do]]\nlight = {{ colour = \"warm\", for = \"5s\" }}\n\
             [permissions]\ntopics = [\"air/indoor\"]\nlight = true\n"
        ),
    )
    .unwrap();
    load(&folder).unwrap()
}

/// Stuffy air every 10 s from `from_s` to `to_s`, the holds kept at each
/// turn as the program's loop keeps them: the seconds the skill ran at.
fn stuffy(engine: &mut Engine, from_s: u64, to_s: u64) -> Vec<u64> {
    let mut ran = Vec::new();
    for at in (from_s..=to_s).step_by(10) {
        let runs = engine.observe("air/indoor", at * 1_000_000, br#"{"co2":1200}"#, None);
        if runs.iter().any(|run| !run.calls.is_empty()) {
            ran.push(at);
        }
        engine.keep_holds(at * 1_000_000);
    }
    ran
}

fn keeping(dir: &Path, skill: lamp_skills::Skill) -> (Engine, PathBuf) {
    let file = dir.join(holds::FILE);
    let mut engine = Engine::new(vec![skill], true);
    engine.keep_holds_in(file.clone(), BOOT.into(), 0).unwrap();
    (engine, file)
}

/// A new program on `boot` at `at_s`, reading what the last one kept.
fn restarted(skill: lamp_skills::Skill, file: &Path, boot: &str, at_s: u64) -> (Engine, usize) {
    let mut engine = Engine::new(vec![skill], true);
    let read = engine
        .keep_holds_in(file.to_path_buf(), boot.into(), at_s * 1_000_000)
        .unwrap();
    (engine, read)
}

/// Held 30 minutes, restarted 20 s later on the same boot: it comes at 50
/// minutes of the hold's first start, as on a lamp that never restarted.
#[test]
fn a_hold_kept_across_a_short_restart_comes_at_its_own_time() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let (mut never, _) = keeping(other.path(), skill(other.path(), ""));
    assert_eq!(stuffy(&mut never, 0, 3_100), [3_000]);

    let (mut engine, file) = keeping(dir.path(), skill(dir.path(), ""));
    assert!(stuffy(&mut engine, 0, 1_800).is_empty());
    drop(engine);
    let (mut engine, read) = restarted(skill(dir.path(), ""), &file, BOOT, 1_820);
    assert_eq!(read, 1);
    assert_eq!(stuffy(&mut engine, 1_820, 3_100), [3_000]);
}

/// Past its gap since it was last seen holding, another boot, or its
/// trigger edited: the hold begins again at the first message after.
#[test]
fn a_hold_begins_again_past_its_gap_on_another_boot_or_edited() {
    for (case, boot, at_s, more, first) in [
        ("past the gap", BOOT, 1_800 + 121, "", 1_930 + 3_000),
        ("another boot", "boot-b", 1_820, "", 1_820 + 3_000),
        ("edited", BOOT, 1_820, "55m|", 1_820 + 3_000),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (mut engine, file) = keeping(dir.path(), skill(dir.path(), ""));
        assert!(stuffy(&mut engine, 0, 1_800).is_empty());
        drop(engine);
        let (mut engine, _) = restarted(skill(dir.path(), more), &file, boot, at_s);
        let from = at_s.div_ceil(10) * 10;
        assert_eq!(stuffy(&mut engine, from, 5_000), [first], "{case}");
    }
}

/// The first message after the restart does not hold: the hold taken back
/// is dropped, whatever its gap, and begins again with the next that does.
#[test]
fn a_hold_taken_back_is_dropped_by_a_first_message_that_does_not_hold() {
    let dir = tempfile::tempdir().unwrap();
    let (mut engine, file) = keeping(dir.path(), skill(dir.path(), ""));
    assert!(stuffy(&mut engine, 0, 1_800).is_empty());
    drop(engine);
    let (mut engine, read) = restarted(skill(dir.path(), ""), &file, BOOT, 1_820);
    assert_eq!(read, 1);
    engine.observe("air/indoor", 1_820_000_000, br#"{"co2":800}"#, None);
    assert_eq!(stuffy(&mut engine, 1_830, 5_000), [1_830 + 3_000]);
}

/// A hold that already ran keeps its cooldown through the restart: never
/// sooner than a lamp that never restarted (again 50 minutes after the
/// run, not at once).
#[test]
fn a_hold_that_ran_keeps_its_cooldown_across_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (mut engine, file) = keeping(dir.path(), skill(dir.path(), ""));
    assert_eq!(stuffy(&mut engine, 0, 3_300), [3_000]);
    drop(engine);
    let (mut engine, _) = restarted(skill(dir.path(), ""), &file, BOOT, 3_320);
    assert_eq!(stuffy(&mut engine, 3_320, 6_100), [6_000]);
}

/// A snoozed skill's hold is not kept: a restart forgets the snooze (it is
/// in memory only), so its hold begins again, as before.
#[test]
fn a_snoozed_skills_hold_is_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let stop = "[stop]\ntool = \"stop_sip\"\nabout = \"When someone asks to stop the sip nudge.\"";
    let (mut engine, file) = keeping(dir.path(), skill(dir.path(), stop));
    assert!(stuffy(&mut engine, 0, 1_000).is_empty());
    engine
        .tool(
            "stop_sip",
            &serde_json::json!({"how": "snooze"}),
            1_000_000_000,
        )
        .unwrap();
    assert!(stuffy(&mut engine, 1_010, 1_800).is_empty());
    drop(engine);
    let (_, read) = restarted(skill(dir.path(), stop), &file, BOOT, 1_820);
    assert_eq!(read, 0);
}
