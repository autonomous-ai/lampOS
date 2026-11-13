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

/// A snoozed skill keeps its hold, and its snooze with it: after the
/// restart it is still snoozed, and does not come at its 50 minutes.
#[test]
fn a_snoozed_skill_stays_snoozed_across_a_restart() {
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
    let (mut engine, read) = restarted(skill(dir.path(), stop), &file, BOOT, 1_820);
    assert_eq!(read, 1, "its hold kept");
    assert!(engine.stopped("sip", 1_820_000_000), "still snoozed");
    assert!(stuffy(&mut engine, 1_820, 6_100).is_empty());
}

/// A wellbeing-shaped nudge on stuffy air: `name`, after `for_m` minutes,
/// again `cooldown_m` after its run, in the group `wellbeing` (two an
/// hour, 30 minutes apart), `top` added before its trigger.
fn member(dir: &Path, name: &str, top: &str, for_m: u32, cooldown_m: u32) -> lamp_skills::Skill {
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
            "{top}\n[trigger]\ntopic = \"air/indoor\"\nfield = \"co2\"\nabove = 1000\n\
             for = \"{for_m}m\"\ngap = \"2m\"\ncooldown = \"{cooldown_m}m\"\n\
             [budget]\ngroup = \"wellbeing\"\nper_hour = 2\ngap = \"30m\"\n\
             [ask]\npolicy = \"nudge\"\n[[do]]\nlight = {{ colour = \"warm\", for = \"5s\" }}\n\
             [permissions]\ntopics = [\"air/indoor\"]\nlight = true\n"
        ),
    )
    .unwrap();
    load(&folder).unwrap()
}

/// Rest-your-eyes (45 minutes) and a sip (50), both of the wellbeing group.
fn wellbeing(dir: &Path) -> Vec<lamp_skills::Skill> {
    vec![
        member(dir, "rest-your-eyes", "", 45, 60),
        member(dir, "sip", "", 50, 50),
    ]
}

/// As [`stuffy`], for several skills: when each ran, and which.
fn stuffy_runs(engine: &mut Engine, from_s: u64, to_s: u64) -> Vec<(u64, String)> {
    let mut ran = Vec::new();
    for at in (from_s..=to_s).step_by(10) {
        for run in engine.observe("air/indoor", at * 1_000_000, br#"{"co2":1200}"#, None) {
            if !run.calls.is_empty() {
                ran.push((at, run.skill));
            }
        }
        engine.keep_holds(at * 1_000_000);
    }
    ran
}

fn keeping_all(dir: &Path, skills: Vec<lamp_skills::Skill>) -> (Engine, PathBuf) {
    let file = dir.join(holds::FILE);
    let mut engine = Engine::new(skills, true);
    engine.keep_holds_in(file.clone(), BOOT.into(), 0).unwrap();
    (engine, file)
}

fn restarted_all(
    skills: Vec<lamp_skills::Skill>,
    file: &Path,
    boot: &str,
    at_s: u64,
) -> (Engine, usize) {
    let mut engine = Engine::new(skills, true);
    let read = engine
        .keep_holds_in(file.to_path_buf(), boot.into(), at_s * 1_000_000)
        .unwrap();
    (engine, read)
}

/// After rest-your-eyes ran at 45 minutes, the sip's hold (due at 50) is
/// kept through a restart in the group's gap, and the sip still waits that
/// gap: it comes 30 minutes after rest-your-eyes, as on a lamp that never
/// restarted, not 50 minutes after the restart (its hold begun again) nor
/// at once (the gap forgotten).
#[test]
fn after_rest_your_eyes_ran_the_sips_hold_is_kept_and_waits_the_group_gap() {
    let other = tempfile::tempdir().unwrap();
    let (mut never, _) = keeping_all(other.path(), wellbeing(other.path()));
    let expected = stuffy_runs(&mut never, 0, 7_000);
    let name = |at: u64, name: &str| (at, name.to_owned());
    assert_eq!(
        expected,
        [
            name(2_700, "rest-your-eyes"),
            name(4_500, "sip"),
            name(6_300, "rest-your-eyes")
        ]
    );

    let dir = tempfile::tempdir().unwrap();
    let (mut engine, file) = keeping_all(dir.path(), wellbeing(dir.path()));
    assert_eq!(stuffy_runs(&mut engine, 0, 3_600), expected[..1]);
    drop(engine);
    let (mut engine, read) = restarted_all(wellbeing(dir.path()), &file, BOOT, 3_620);
    assert_eq!(read, 2, "both holds kept, the group's run within the hour");
    assert_eq!(stuffy_runs(&mut engine, 3_620, 7_000), expected[1..]);
}

/// While the sip waits out the group's gap, held back at every message,
/// the file is written on the cadence, not at each message: when it was
/// last held back changes then, and nothing else.
#[test]
fn a_member_waiting_out_the_group_gap_is_written_on_the_cadence() {
    let dir = tempfile::tempdir().unwrap();
    let (mut engine, file) = keeping_all(dir.path(), wellbeing(dir.path()));
    let (mut writes, mut last, mut turns) = (0u32, Vec::new(), 0u32);
    for at in 0..=4_400u64 {
        engine.observe("air/indoor", at * 1_000_000, br#"{"co2":1200}"#, None);
        engine.keep_holds(at * 1_000_000);
        if at >= 3_100 {
            turns += 1;
            let now = fs::read(&file).unwrap_or_default();
            if now != last {
                writes += 1;
                last = now;
            }
        }
    }
    assert!(writes <= turns / 30 + 2, "{writes} writes in {turns} turns");
}

/// A skill at its day's most stays at its most after a restart: its hold
/// kept, it does not come again that day.
#[test]
fn a_skill_at_its_days_most_stays_at_its_most_after_a_restart() {
    let sip = |dir: &Path| vec![member(dir, "sip", "most_a_day = 1", 50, 50)];
    let dir = tempfile::tempdir().unwrap();
    let (mut engine, file) = keeping_all(dir.path(), sip(dir.path()));
    assert_eq!(stuffy_runs(&mut engine, 0, 3_100).len(), 1);
    drop(engine);
    let (mut engine, read) = restarted_all(sip(dir.path()), &file, BOOT, 3_120);
    assert_eq!(read, 1, "its hold kept");
    assert!(stuffy_runs(&mut engine, 3_120, 9_000).is_empty());
}

/// Another boot takes back nothing, neither the hold nor the day's count:
/// the hold begins again at the restart and the skill comes 50 minutes on.
#[test]
fn another_boot_restores_nothing() {
    let sip = |dir: &Path| vec![member(dir, "sip", "most_a_day = 1", 50, 50)];
    let dir = tempfile::tempdir().unwrap();
    let (mut engine, file) = keeping_all(dir.path(), sip(dir.path()));
    assert_eq!(stuffy_runs(&mut engine, 0, 3_100).len(), 1);
    drop(engine);
    let (mut engine, read) = restarted_all(sip(dir.path()), &file, "boot-b", 3_120);
    assert_eq!(read, 0);
    let ran: Vec<u64> = stuffy_runs(&mut engine, 3_120, 7_000)
        .into_iter()
        .map(|(at, _)| at)
        .collect();
    assert_eq!(ran, [3_120 + 3_000]);
}

/// A day's count kept from yesterday is dropped: past the local midnight
/// the skill comes again at its cooldown, as on a lamp that never
/// restarted, and the old count is not written again.
#[test]
fn another_days_count_is_dropped() {
    // 23:00 UTC on a day, the local time UTC.
    const LATE_MS: u64 = 1_762_383_600_000;
    let sip = |dir: &Path| vec![member(dir, "sip", "most_a_day = 1", 50, 50)];
    let clocked = |dir: &Path, boot_at_s: u64, file: &Path| {
        let mut engine = Engine::new(sip(dir), true);
        engine.clock(0, 0, LATE_MS);
        let read = engine
            .keep_holds_in(file.to_path_buf(), BOOT.into(), boot_at_s * 1_000_000)
            .unwrap();
        (engine, read)
    };
    let other = tempfile::tempdir().unwrap();
    let (mut never, _) = clocked(other.path(), 0, &other.path().join(holds::FILE));
    let expected: Vec<u64> = stuffy_runs(&mut never, 0, 6_100)
        .into_iter()
        .map(|(at, _)| at)
        .collect();
    assert_eq!(expected, [3_000, 6_000], "the second on the next day");

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(holds::FILE);
    let (mut engine, _) = clocked(dir.path(), 0, &file);
    // Stopped at 23:59:50, the file written last before midnight.
    assert_eq!(stuffy_runs(&mut engine, 0, 3_590).len(), 1);
    let kept: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        kept["limits"]["skills"]["sip"]["day"][1], 1,
        "today's count"
    );
    drop(engine);
    // 00:00:10 the next day, within the hold's gap.
    let (mut engine, read) = clocked(dir.path(), 3_610, &file);
    assert_eq!(read, 1);
    assert!(stuffy_runs(&mut engine, 3_610, 3_800).is_empty());
    let kept: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert!(
        kept["limits"]["skills"]["sip"]["day"].is_null(),
        "yesterday's count not written again: {kept}"
    );
    let ran: Vec<u64> = stuffy_runs(&mut engine, 3_810, 6_100)
        .into_iter()
        .map(|(at, _)| at)
        .collect();
    assert_eq!(ran, expected[1..]);
}

/// A read of a linked service (#5288) after someone has been here 2
/// minutes: its once a day is in memory only, as a snooze is.
fn reader(dir: &Path) -> lamp_skills::Skill {
    let folder = dir.join("evening-calendar");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        "---\nname: evening-calendar\ndescription: A test.\n---\n",
    )
    .unwrap();
    fs::write(
        folder.join("skill.toml"),
        "[trigger]\ntopic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\
         for = \"2m\"\ngap = \"1m\"\n[ask]\npolicy = \"nudge\"\n[permissions]\n\
         topics = [\"room/presence\"]\nspeak = true\n\
         services = { google_calendar = \"read\" }\n[[do]]\n\
         read = { service = \"google_calendar\", what = \"events\", day = \"tomorrow\" }\n",
    )
    .unwrap();
    load(&folder).unwrap()
}

/// Someone here, or not, every 10 s from `from_s` to `to_s`, the holds kept
/// at each turn: the seconds the read was made at.
fn here(engine: &mut Engine, from_s: u64, to_s: u64, someone: bool) -> Vec<u64> {
    let mut ran = Vec::new();
    let payload = format!("{{\"someone\":{someone}}}");
    for at in (from_s..=to_s).step_by(10) {
        let runs = engine.observe("room/presence", at * 1_000_000, payload.as_bytes(), None);
        if runs.iter().any(|run| !run.calls.is_empty()) {
            ran.push(at);
        }
        engine.keep_holds(at * 1_000_000);
    }
    ran
}

/// A read's hold is kept until its read is made; from then till its local
/// midnight none is (the read under way, done, and a new sitting the same
/// day), so a restart never gives its day's chance back sooner than a hold
/// begun again (the readers' P3 on #6295).
#[test]
fn a_reads_hold_is_not_kept_once_its_read_came_due_today() {
    // 10:00 UTC on a day, the local time UTC.
    const MORNING_MS: u64 = 1_762_336_800_000;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(holds::FILE);
    let mut engine = Engine::new(vec![reader(dir.path())], true);
    engine.clock(0, 0, MORNING_MS);
    engine.keep_holds_in(file.clone(), BOOT.into(), 0).unwrap();
    let kept = |at_s: u64| restarted(reader(dir.path()), &file, BOOT, at_s).1;
    assert!(here(&mut engine, 0, 60, true).is_empty());
    assert_eq!(kept(65), 1, "its read still to come: kept");
    assert_eq!(here(&mut engine, 70, 130, true), [120]);
    assert_eq!(kept(135), 0, "its read under way");
    engine.done("evening-calendar");
    assert!(here(&mut engine, 140, 200, true).is_empty());
    assert_eq!(kept(205), 0, "its read done today");
    assert!(here(&mut engine, 210, 290, false).is_empty());
    assert!(here(&mut engine, 300, 400, true).is_empty());
    assert_eq!(kept(405), 0, "a new sitting, its read made today");
}

/// The tools' folder is kept through a restart for the holds, and the last
/// run's `tools.json` with it: let go before the new run's `call.sock` is
/// bound, so the voice is never offered a tool from before (an update's
/// dropped one), and written again from what the first load found (the
/// readers' P3 on #6295).
#[test]
fn a_kept_folders_tools_list_is_written_again_at_the_start() {
    use std::{
        process::{Child, Command},
        time::{Duration, Instant},
    };
    struct Running(Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    // Short: a socket's whole path has to fit in 103 bytes.
    let dir = tempfile::Builder::new()
        .prefix("hk")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path();
    let skills = root.join("skills");
    let folder = skills.join("wave");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        "---\nname: wave\ndescription: A test.\n---\n",
    )
    .unwrap();
    fs::write(
        folder.join("skill.toml"),
        "tool = \"wave\"\n[[do]]\nsay = \"Say hello.\"\n[permissions]\nspeak = true\n",
    )
    .unwrap();
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    let stale = r#"{"tools":[{"name":"old_wave","description":"Gone in the update."}]}"#;
    fs::write(tools.join("tools.json"), stale).unwrap();
    let _running = Running(
        Command::new(env!("CARGO_BIN_EXE_lamp-skills"))
            .args([
                "--dir".as_ref(),
                skills.as_os_str(),
                "--bus-root".as_ref(),
                root.join("bus").as_os_str(),
                "--actions-socket".as_ref(),
                root.join("actions.sock").as_os_str(),
                "--tools-dir".as_ref(),
                tools.as_os_str(),
                "--boot-id".as_ref(),
                BOOT.as_ref(),
            ])
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !tools.join("call.sock").exists() {
        assert!(Instant::now() < deadline, "no call.sock");
        std::thread::sleep(Duration::from_millis(20));
    }
    let read = || fs::read_to_string(tools.join("tools.json")).ok();
    assert_ne!(read().as_deref(), Some(stale), "the old list let go first");
    let file = loop {
        if let Some(text) = read() {
            break serde_json::from_str::<serde_json::Value>(&text).unwrap();
        }
        assert!(Instant::now() < deadline, "no tools file");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        file,
        serde_json::json!({"tools":[{"name":"wave","description":"A test."}]})
    );
}
