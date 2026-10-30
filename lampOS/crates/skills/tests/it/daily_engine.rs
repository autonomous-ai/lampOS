//! Local windows and durable daily chances, through the real skill loader
//! and engine. No lamp or wall-clock wait is needed.
use lamp_msgs::Faces;
use lamp_skills::{Call, Engine, LOCAL_CLOCK_TOPIC, Run, Skill, load, load_all};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

const MIDNIGHT: u64 = 1_767_225_600_000; // 2026-01-01 UTC.

fn private_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root
}

// Fixtures for clock/storage behavior declare the inputs and outputs they
// exercise; permission refusal has its own loader tests.
fn granted(text: &str) -> String {
    if text.contains("[permissions]") {
        text.to_owned()
    } else {
        format!(
            "{text}\n[permissions]\ntopics = [\"clock/local\", \"voice/state\", \"humans/faces/tracked\", \"room/presence\", \"fixture/state\"]\nbody = true\nspeak = true\noutput_limits = true\n"
        )
    }
}

fn manifest(root: &Path, text: &str) -> Skill {
    let folder = root.join("daily-check");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        "---\nname: daily-check\ndescription: A small scheduled movement.\n---\n",
    )
    .unwrap();
    fs::write(folder.join("skill.toml"), granted(text)).unwrap();
    load(&folder).unwrap()
}

fn skill(root: &Path, enabled: &str, trigger: &str) -> Skill {
    manifest(
        root,
        &format!(
            "{enabled}\n[trigger]\n{trigger}\n[ask]\npolicy = \"nudge\"\n\n[[do]]\nbody = {{ gesture = \"stretching\" }}\n"
        ),
    )
}

fn clock(engine: &mut Engine, monotonic_us: u64, day: u64, minute: u64) {
    engine.clock(
        0,
        monotonic_us,
        MIDNIGHT + day * 86_400_000 + minute * 60_000,
    );
}

fn calls(runs: Vec<Run>) -> Vec<Call> {
    runs.into_iter().flat_map(|run| run.calls).collect()
}

fn morning() -> &'static str {
    "name = \"morning\"\ntopic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\nduring = { from = \"06:00\", to = \"10:00\" }\nonce_per_day = true"
}

#[test]
fn disabled_skills_are_validated_but_never_started_or_offered() {
    let root = private_root();
    let off = skill(root.path(), "enabled = false", morning());
    assert!(!off.enabled);
    let (loaded, errors) = load_all(root.path());
    assert!(loaded.is_empty());
    assert!(errors.is_empty());
    let mut engine = Engine::new(vec![off], true);
    clock(&mut engine, 0, 0, 360);
    assert!(engine.clock_tick(0).is_empty());
    assert!(engine.skills().is_empty());
    assert!(engine.topics().is_empty());
    let on = skill(root.path(), "", morning());
    assert!(on.enabled, "old manifests retain their default");
}

#[test]
fn core_bedtime_manifest_is_disabled_and_has_no_model_function() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/bedtime-morning");
    let skill = load(&path).unwrap();
    assert!(!skill.enabled);
    assert!(skill.is_routine());
    assert!(skill.tool.is_none());
    // Its rest and wake are Night sleep's now (#940): the evening glow and
    // the morning hum are left.
    assert_eq!(skill.triggers.len(), 2);
    // A rest needs the body, which it no longer has.
    assert!(!skill.permissions.body);
    assert!(skill.triggers.iter().all(|trigger| trigger.once_per_day));
    // The morning greeting (the first builder's ruling): 06:00 to 11:00,
    // and not once the person has spoken to the lamp since 06:00.
    let morning = skill
        .triggers
        .iter()
        .find(|trigger| trigger.name.as_deref() == Some("morning"))
        .unwrap();
    let window = morning.during.unwrap();
    assert!(!window.contains(5 * 60 + 59) && window.contains(6 * 60));
    assert!(window.contains(10 * 60 + 59) && !window.contains(11 * 60));
    assert!(morning.unless_heard);
    assert!(
        skill
            .triggers
            .iter()
            .filter(|trigger| trigger.unless_heard)
            .count()
            == 1
    );
    assert!(skill.permissions.refuses_reading("voice/heard").is_none());
    assert!(
        lamp_skills::tools_file(&[skill])["tools"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn disabled_tool_is_excluded_even_when_passed_directly_to_the_catalogue() {
    let root = private_root();
    let skill = manifest(
        root.path(),
        "enabled = false\ntool = \"small_stretch\"\n[[do]]\nbody = { gesture = \"stretching\" }\n",
    );
    assert!(
        lamp_skills::tools_file(&[skill])["tools"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unknown_rest_values_and_unsafe_caps_are_refused_at_load() {
    let root = private_root();
    let path = root.path().join("daily-check");
    let base = "[trigger]\ntopic = \"clock/local\"\nfield = \"known\"\nequals = true\n[ask]\npolicy = \"nudge\"\n[[do]]\n";
    manifest(
        root.path(),
        &format!("{base}rest = {{ op = \"cancel\" }}\n"),
    );
    for step in [
        "rest = { op = \"future\" }",
        "rest = { op = \"rest\" }",
        "rest = { op = \"wake\", during = { from = \"06:00\", to = \"10:00\" } }",
        "cue = \"alarm\"",
    ] {
        fs::write(path.join("skill.toml"), granted(&format!("{base}{step}\n"))).unwrap();
        let error = load(&path).unwrap_err();
        assert!(!error.contains("not permitted"), "{step}: {error}");
    }
    fs::write(path.join("skill.toml"), granted(&format!("[limits]\nduring = {{ from = \"22:00\", to = \"06:00\" }}\nvolume_raw_max = 0\n{base}rest = {{ op = \"cancel\" }}\n"))).unwrap();
    assert!(load(&path).unwrap_err().contains("limits require"));
}

#[test]
fn routine_quiet_time_restarts_after_a_stale_voice_heartbeat() {
    let root = private_root();
    let text = format!(
        "[trigger]\n{}\nwait = \"1m\"\n[[while]]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\nfor = \"2s\"\n[ask]\npolicy = \"nudge\"\n[[do]]\nrest = {{ op = \"cancel\" }}\n",
        morning()
    );
    let loaded = manifest(root.path(), &text);
    let mut engine = Engine::new(vec![loaded], true)
        .with_daily_journal(crate::journal(root.path().join("daily.json")));
    clock(&mut engine, 0, 0, 360);
    engine.observe("voice/state", 0, br#"{"state":"resting"}"#, None);
    assert!(calls(engine.clock_tick(0)).is_empty());
    assert!(
        calls(engine.clock_tick(5_000_000)).is_empty(),
        "one old resting message cannot spend the daily chance"
    );
    for now in [5_000_000, 6_000_000] {
        engine.observe("voice/state", now, br#"{"state":"resting"}"#, None);
        assert!(
            calls(engine.clock_tick(now)).is_empty(),
            "quiet duration must restart"
        );
    }
    engine.observe("voice/state", 7_000_000, br#"{"state":"resting"}"#, None);
    assert_eq!(calls(engine.clock_tick(7_000_000)).len(), 1);
}

#[test]
fn generic_routines_subscribe_to_the_voice_their_final_gate_requires() {
    let root = private_root();
    let loaded = manifest(
        root.path(),
        "[trigger]\ntopic = \"clock/local\"\nfield = \"known\"\nequals = true\n[ask]\npolicy = \"nudge\"\n[[do]]\nrest = { op = \"cancel\" }\n",
    );
    let mut engine = Engine::new(vec![loaded], true);
    assert!(engine.topics().contains(&"voice/state".into()));
    assert!(calls(engine.clock_tick(0)).is_empty());
}

#[test]
fn a_daily_chance_requires_both_a_sane_clock_and_a_durable_journal() {
    let root = private_root();
    let loaded = skill(root.path(), "", morning());
    let mut engine = Engine::new(vec![loaded.clone()], true);
    assert!(calls(engine.clock_tick(0)).is_empty());
    engine.clock(0, 0, 360 * 60_000);
    assert!(calls(engine.clock_tick(0)).is_empty(), "unset epoch clock");
    clock(&mut engine, 0, 0, 360);
    let refused = engine.clock_tick(0);
    assert!(refused.iter().any(|run| {
        run.events
            .iter()
            .any(|event| event.allowed == Some(false) && event.why.contains("writable journal"))
    }));
    assert!(calls(refused).is_empty());
    let journal = crate::journal(root.path().join("daily.json"));
    let mut engine = Engine::new(vec![loaded], true).with_daily_journal(journal);
    clock(&mut engine, 0, 0, 360);
    assert_eq!(calls(engine.clock_tick(0)).len(), 1);
}

#[test]
fn corrected_dates_run_once_after_a_future_claim_restart_and_reload() {
    let root = private_root();
    let loaded = skill(root.path(), "", morning());
    let path = root.path().join("daily.json");
    {
        let mut engine =
            Engine::new(vec![loaded.clone()], true).with_daily_journal(crate::journal(&path));
        clock(&mut engine, 0, 0, 360);
        assert_eq!(calls(engine.clock_tick(0)).len(), 1);
        clock(&mut engine, 1, 20_000, 300);
        engine.clock_tick(1);
        clock(&mut engine, 2, 20_000, 360);
        assert_eq!(calls(engine.clock_tick(2)).len(), 1);
        assert!(engine.replace(vec![loaded.clone()]).is_empty());
        assert!(calls(engine.clock_tick(3)).is_empty());
    }
    {
        let mut engine =
            Engine::new(vec![loaded.clone()], true).with_daily_journal(crate::journal(&path));
        clock(&mut engine, 0, 0, 370);
        assert!(
            calls(engine.clock_tick(0)).is_empty(),
            "the real day was already used"
        );
        clock(&mut engine, 1, 1, 300);
        engine.clock_tick(1);
        clock(&mut engine, 2, 1, 360);
        assert_eq!(
            calls(engine.clock_tick(2)).len(),
            1,
            "the corrected next date was never used"
        );
    }
    let mut engine = Engine::new(vec![loaded], true).with_daily_journal(crate::journal(&path));
    for (index, day) in [0, 1, 20_000].into_iter().enumerate() {
        let now = (index * 2) as u64;
        clock(&mut engine, now, day, 300);
        engine.clock_tick(now);
        clock(&mut engine, now + 1, day, 360);
        assert!(
            calls(engine.clock_tick(now + 1)).is_empty(),
            "claimed day {day}"
        );
    }
    clock(&mut engine, 6, 2, 300);
    engine.clock_tick(6);
    clock(&mut engine, 7, 2, 360);
    assert_eq!(calls(engine.clock_tick(7)).len(), 1);
}

#[test]
fn an_unsynchronized_clock_neither_spends_a_day_nor_approves_pending_output() {
    let root = private_root();
    let loaded = skill(root.path(), "", morning());
    let path = root.path().join("daily.json");
    let mut engine = Engine::new(vec![loaded], false)
        .asking_the_policy()
        .with_daily_journal(crate::journal(&path));
    assert!(engine.needs_daily_clock());
    let local = lamp_local_time::LocalTime::Offset(0);
    let wall = MIDNIGHT + 360 * 60_000;
    for (index, synchronized) in [None, Some(false)].into_iter().enumerate() {
        let trusted = lamp_skills::daily::trusted_unix_ms(wall, synchronized);
        engine.clock_in_with_trust(&local, index as u64, wall, trusted.is_some());
        assert!(engine.clock_tick(index as u64).is_empty());
        assert!(!path.exists(), "unknown time must not consume the day");
    }
    engine.clock_in_with_trust(&local, 2, wall, true);
    let run = engine
        .clock_tick(2)
        .pop()
        .expect("a synchronized date can ask once");
    assert!(run.ask.is_some());
    assert!(engine.still(&run, 2).is_some());
    engine.clock_in_with_trust(&local, 3, wall, false);
    assert!(
        engine.still(&run, 3).is_none(),
        "trust loss invalidates the last good sample"
    );
    // The morning's watch, held since 2, is reset, and the journal says so.
    assert_eq!(
        reasons(engine.clock_tick(3)),
        vec!["morning watch reset after 0 s: no trusted local moment"]
    );
    engine.clock_in_with_trust(&local, 4, wall, true);
    assert!(
        engine.clock_tick(4).is_empty(),
        "trust restoration does not refund the claimed day"
    );
}

#[test]
fn disabled_daily_skills_and_ordinary_quiet_hours_keep_their_existing_behavior() {
    let root = private_root();
    let disabled = skill(root.path(), "enabled = false", morning());
    let disabled = Engine::new(vec![disabled], true);
    assert!(!disabled.needs_daily_clock());
    let ordinary = manifest(
        root.path(),
        "[trigger]\ntopic = \"fixture/state\"\nfield = \"ready\"\nequals = true\n[quiet]\nfrom = \"22:00\"\nto = \"08:00\"\n[ask]\npolicy = \"nudge\"\n[[do]]\nbody = { gesture = \"stretching\" }\n",
    );
    let mut engine = Engine::new(vec![ordinary], true);
    assert!(!engine.needs_daily_clock());
    let local = lamp_local_time::LocalTime::Offset(0);
    engine.clock_in_with_trust(&local, 0, MIDNIGHT + 6 * 3_600_000, false);
    assert!(calls(engine.observe("fixture/state", 0, br#"{"ready":true}"#, None)).is_empty());
    engine.clock_in_with_trust(&local, 1, MIDNIGHT + 10 * 3_600_000, false);
    assert_eq!(
        calls(engine.observe("fixture/state", 1, br#"{"ready":true}"#, None)).len(),
        1
    );
}

#[test]
fn the_daily_claim_precedes_a_policy_answer_and_is_not_replayed_after_refusal() {
    let root = private_root();
    let loaded = skill(root.path(), "", morning());
    let path = root.path().join("daily.json");
    let run = {
        let mut engine = Engine::new(vec![loaded.clone()], false)
            .asking_the_policy()
            .with_daily_journal(crate::journal(&path));
        clock(&mut engine, 0, 0, 360);
        let run = engine.clock_tick(0).pop().unwrap();
        assert!(run.ask.is_some());
        clock(&mut engine, 1, 0, 600);
        assert!(
            engine.still(&run, 1).is_none(),
            "window ended while Policy answered"
        );
        run
    };
    assert!(!run.calls.is_empty());
    let mut restarted = Engine::new(vec![loaded], true).with_daily_journal(crate::journal(path));
    clock(&mut restarted, 0, 0, 361);
    assert!(calls(restarted.clock_tick(0)).is_empty());
}

#[test]
fn a_missing_camera_is_not_absence_and_time_outside_the_window_does_not_count() {
    let root = private_root();
    let loaded = skill(
        root.path(),
        "",
        "topic = \"humans/faces/tracked\"\npresent = false\nfor = \"2s\"\nrearm = \"ends\"\nduring = { from = \"23:00\", to = \"06:00\" }",
    );
    let mut engine = Engine::new(vec![loaded], true);
    let empty = br#"{"faces":[]}"#;
    clock(&mut engine, 0, 0, 1379);
    assert!(calls(engine.observe(Faces::TOPIC, 0, empty, None)).is_empty());
    clock(&mut engine, 1_000_000, 0, 1380);
    assert!(calls(engine.observe(Faces::TOPIC, 1_000_000, b"{}", None)).is_empty());
    assert!(calls(engine.observe(Faces::TOPIC, 2_000_000, empty, None)).is_empty());
    assert!(calls(engine.observe(Faces::TOPIC, 3_000_000, empty, None)).is_empty());
    assert_eq!(
        calls(engine.observe(Faces::TOPIC, 4_000_000, empty, None)).len(),
        1
    );
}

#[test]
fn a_gap_in_the_empty_face_stream_restarts_the_absence_duration() {
    let root = private_root();
    let loaded = skill(
        root.path(),
        "",
        "topic = \"humans/faces/tracked\"\npresent = false\nfor = \"2s\"\nrearm = \"ends\"",
    );
    let mut engine = Engine::new(vec![loaded], true);
    let empty = br#"{"faces":[]}"#;
    assert!(calls(engine.observe(Faces::TOPIC, 0, empty, None)).is_empty());
    assert!(calls(engine.observe(Faces::TOPIC, 3_000_000, empty, None)).is_empty());
    assert!(calls(engine.observe(Faces::TOPIC, 4_000_000, empty, None)).is_empty());
    assert_eq!(
        calls(engine.observe(Faces::TOPIC, 5_000_000, empty, None)).len(),
        1
    );
}

#[test]
fn clock_triggers_use_no_external_topic_and_an_expired_clock_cannot_start_them() {
    let root = private_root();
    let loaded = skill(
        root.path(),
        "",
        "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nduring = { from = \"06:00\", to = \"10:00\" }",
    );
    let mut engine = Engine::new(vec![loaded], true);
    clock(&mut engine, 0, 0, 360);
    assert!(
        engine
            .topics()
            .iter()
            .any(|topic| topic == LOCAL_CLOCK_TOPIC)
    );
    assert_eq!(calls(engine.clock_tick(0)).len(), 1);
    assert!(calls(engine.clock_tick(120_000_001)).is_empty());
}

#[test]
fn an_ordinary_pending_policy_run_revalidates_across_an_unrelated_reload() {
    let root = private_root();
    let trigger = "topic = \"room/presence\"\nfield = \"present\"\nequals = true";
    let loaded = skill(root.path(), "", trigger);
    let mut engine = Engine::new(vec![loaded.clone()], false).asking_the_policy();
    let run = engine
        .observe("room/presence", 0, br#"{"present":true}"#, Some(1_000))
        .pop()
        .unwrap();
    assert!(run.ask.is_some());
    assert_eq!(run.calls, [Call::Gesture("stretching".into())]);

    let mut unrelated = loaded.clone();
    unrelated.name = "another-skill".into();
    assert!(engine.replace(vec![unrelated, loaded]).is_empty());
    assert_eq!(engine.still(&run, 1), Some(run.calls.clone()));
    engine.observe("room/presence", 2, br#"{"present":false}"#, Some(1_000));
    assert!(
        engine.still(&run, 2).is_none(),
        "the original condition still matters"
    );
    engine.observe("room/presence", 3, br#"{"present":true}"#, Some(1_000));
    assert_eq!(engine.still(&run, 3), Some(run.calls.clone()));

    // An old ordinary approval cannot become permission for new scheduled work.
    let scheduled = skill(
        root.path(),
        "",
        &format!("{trigger}\nduring = {{ from = \"06:00\", to = \"10:00\" }}"),
    );
    clock(&mut engine, 3, 0, 360);
    assert!(engine.replace(vec![scheduled]).is_empty());
    assert!(engine.still(&run, 3).is_none());
}

#[test]
fn windowed_and_routine_policy_runs_keep_their_definition_fence() {
    for routine in [false, true] {
        let root = private_root();
        let window = if routine {
            ""
        } else {
            "during = { from = \"06:00\", to = \"10:00\" }"
        };
        let step = if routine {
            "rest = { op = \"cancel\" }"
        } else {
            "body = { gesture = \"stretching\" }"
        };
        let loaded = manifest(
            root.path(),
            &format!(
                "[trigger]\ntopic = \"room/presence\"\nfield = \"present\"\nequals = true\n{window}\n[ask]\npolicy = \"nudge\"\n[[do]]\n{step}\n"
            ),
        );
        let mut engine = Engine::new(vec![loaded.clone()], false).asking_the_policy();
        clock(&mut engine, 0, 0, 360);
        engine.observe("voice/state", 0, br#"{"state":"resting"}"#, None);
        let run = engine
            .observe("room/presence", 0, br#"{"present":true}"#, Some(1_000))
            .pop()
            .unwrap();
        assert!(run.ask.is_some());
        assert!(!run.calls.is_empty());
        assert_eq!(engine.still(&run, 1), Some(run.calls.clone()));
        assert!(engine.replace(vec![loaded]).is_empty());
        assert!(
            engine.still(&run, 1).is_none(),
            "scheduled kind: routine={routine}"
        );
    }
}

#[test]
fn a_pending_daily_ask_cannot_be_rebound_to_a_reordered_trigger() {
    let root = private_root();
    let immediate = morning().replace("\"morning\"", "\"immediate\"");
    let delayed = format!(
        "{}\nfor = \"1h\"",
        morning().replace("\"morning\"", "\"delayed\"")
    );
    let definition = |first: &str, second: &str| {
        format!(
            "[[trigger]]\n{first}\n[[trigger]]\n{second}\n\
             [ask]\npolicy = \"nudge\"\n\
             [[do]]\non = \"immediate\"\nbody = {{ gesture = \"stretching\" }}\n\
             [[do]]\non = \"delayed\"\nbody = {{ gesture = \"nod\" }}\n"
        )
    };
    let loaded = manifest(root.path(), &definition(&immediate, &delayed));
    let mut engine = Engine::new(vec![loaded], false)
        .asking_the_policy()
        .with_daily_journal(crate::journal(root.path().join("daily.json")));
    clock(&mut engine, 0, 0, 360);
    let mut pending = engine.clock_tick(0);
    assert_eq!(
        pending.len(),
        1,
        "the delayed trigger has not held for an hour"
    );
    let run = pending.pop().unwrap();
    assert!(run.ask.is_some());
    assert_eq!(run.calls, [Call::Gesture("stretching".into())]);
    assert_eq!(engine.still(&run, 1), Some(run.calls.clone()));

    let reordered = manifest(root.path(), &definition(&delayed, &immediate));
    assert!(engine.replace(vec![reordered]).is_empty());
    assert!(
        engine.still(&run, 1).is_none(),
        "an old ask must not run the unclaimed delayed trigger now at index zero"
    );
    assert!(
        engine.clock_tick(1).is_empty(),
        "reload does not refund the immediate trigger's consumed day"
    );
    clock(&mut engine, 2, 1, 300);
    // Only the line saying the held watch left its window.
    let left = engine.clock_tick(2);
    assert!(
        left.iter()
            .all(|run| run.calls.is_empty() && run.ask.is_none())
    );
    assert!(
        reasons(left)
            .iter()
            .all(|line| line.contains("watch reset") && line.contains("left its window"))
    );
    clock(&mut engine, 3, 1, 360);
    let next = engine.clock_tick(3);
    assert_eq!(next.len(), 1);
    assert_eq!(engine.still(&next[0], 3), Some(run.calls));
}

#[test]
fn a_missing_journal_reports_once_across_ticks_and_reload_until_one_is_installed() {
    let root = private_root();
    // No rearm: every clock tick would otherwise repeat the storage refusal.
    let daily = morning().replace("rearm = \"ends\"\n", "");
    let loaded = manifest(
        root.path(),
        &format!(
            "[[trigger]]\n{daily}\n\
             [[trigger]]\nname = \"ordinary\"\ntopic = \"fixture/state\"\n\
             field = \"ready\"\nequals = true\n\
             [ask]\npolicy = \"nudge\"\n\
             [[do]]\nbody = {{ gesture = \"stretching\" }}\n"
        ),
    );
    let mut engine = Engine::new(vec![loaded.clone()], true);
    clock(&mut engine, 0, 0, 360);
    let first = engine.clock_tick(0);
    assert_eq!(first.len(), 1);
    assert!(first[0].calls.is_empty());
    assert!(first[0].ask.is_none());
    assert_eq!(first[0].events.len(), 1);
    assert_eq!(first[0].events[0].allowed, Some(false));
    assert!(first[0].events[0].why.contains("writable journal"));
    for at_us in [1, 250_000, 500_000] {
        assert!(engine.clock_tick(at_us).is_empty());
    }
    // Each later chance is dropped by the fault, said once (#2602).
    assert_eq!(
        engine.take_dropped(),
        ["daily-check not run: its daily journal is faulted"]
    );
    assert!(engine.replace(vec![loaded]).is_empty());
    assert!(engine.clock_tick(500_001).is_empty());
    assert_eq!(
        calls(engine.observe("fixture/state", 500_002, br#"{"ready":true}"#, None)).len(),
        1,
        "ordinary triggers continue after a daily fault"
    );
    clock(&mut engine, 500_003, 1, 360);
    assert!(
        engine.clock_tick(500_003).is_empty(),
        "a new day is no repair"
    );
    engine = engine.with_daily_journal(crate::journal(root.path().join("daily.json")));
    assert_eq!(calls(engine.clock_tick(500_004)).len(), 1);
    assert!(engine.clock_tick(500_005).is_empty());
}

#[test]
fn a_storage_fault_withholds_pending_daily_output_and_is_not_retried() {
    let root = private_root();
    let daily = morning().replace("rearm = \"ends\"\n", "");
    let loaded = skill(root.path(), "", &daily);
    let mut engine = Engine::new(vec![loaded], false)
        .asking_the_policy()
        .with_daily_journal(crate::journal(root.path().join("daily.json")));
    clock(&mut engine, 0, 0, 360);
    let run = engine.clock_tick(0).pop().unwrap();
    assert!(engine.still(&run, 1).is_some());

    // Changing the trusted directory to an invalid mode deterministically
    // fails the journal's ownership check before a write.
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let failed = engine.clock_tick(1);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].events[0].allowed, Some(false));
    assert!(failed[0].calls.is_empty());
    assert!(failed[0].ask.is_none());
    assert!(engine.still(&run, 1).is_none());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    clock(&mut engine, 2, 1, 360);
    assert!(
        engine.clock_tick(2).is_empty(),
        "repair alone does not clear the latch"
    );
    assert!(engine.clock_tick(3).is_empty());
}

/// Row 7's try 3 on lamp-06c1 (#1672): the empty desk matured while the
/// voice had not yet rested its 30 s. Bedtime's `wait` keeps that chance;
/// without it the chance is spent, and the journal says so.
#[test]
fn bedtime_keeps_a_chance_that_matured_while_the_voice_was_listening() {
    for wait in [true, false] {
        let root = private_root();
        let text = format!(
            "[[trigger]]\nname = \"bedtime\"\ntopic = \"humans/faces/tracked\"\npresent = false\nfor = \"1m\"\n{}rearm = \"ends\"\nduring = {{ from = \"23:00\", to = \"06:00\" }}\nonce_per_day = true\n[[while]]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\nfor = \"30s\"\n[ask]\npolicy = \"nudge\"\n[[do]]\non = \"bedtime\"\nrest = {{ op = \"rest\", during = {{ from = \"23:00\", to = \"10:00\" }} }}\n",
            if wait { "wait = \"15m\"\n" } else { "" }
        );
        let loaded = manifest(root.path(), &text);
        let mut engine = Engine::new(vec![loaded], true)
            .with_daily_journal(crate::journal(root.path().join("daily.json")));
        let empty = br#"{"faces":[]}"#;
        let mut fired = Vec::new();
        let mut dropped = false;
        for second in 0..=120_u64 {
            let now = second * 1_000_000;
            clock(&mut engine, now, 0, 23 * 60 + second / 60);
            let voice: &[u8] = if second == 55 {
                br#"{"state":"listening"}"#
            } else {
                br#"{"state":"resting"}"#
            };
            engine.observe("voice/state", now, voice, None);
            let runs = engine.observe(Faces::TOPIC, now, empty, None);
            dropped |= runs.iter().any(|run| {
                run.events
                    .iter()
                    .any(|event| event.why.contains("bedtime chance dropped"))
            });
            if !calls(runs).is_empty() {
                fired.push(second);
            }
        }
        if wait {
            // Rested again from 56 s: 30 s later, and once.
            assert_eq!(fired, vec![86]);
            assert!(!dropped);
        } else {
            assert!(fired.is_empty(), "{fired:?}");
            assert!(dropped, "a dropped chance says so in the journal");
        }
    }
}

/// #1672: the evening glow is relative to the person's light, and a rest
/// a sound ended settles again; both are checked as the manifest loads.
#[test]
fn relative_lights_and_resettles_are_checked_at_load() {
    let root = private_root();
    let path = root.path().join("daily-check");
    let base = "[trigger]\ntopic = \"clock/local\"\nfield = \"known\"\nequals = true\n[ask]\npolicy = \"nudge\"\n[[do]]\n";
    let granted_light = |step: &str| {
        format!(
            "[permissions]\ntopics = [\"clock/local\", \"voice/state\"]\nbody = true\nlight = true\n{base}{step}\n"
        )
    };
    manifest(
        root.path(),
        &granted_light(
            "light = { relative = { warmer = 1, dimmer = 1 }, for = \"20s\", pattern = { kind = \"breathe\", period = \"10s\" } }",
        ),
    );
    // #3691: the warmth alone at the dimmest level, and a light never
    // brighter than the person's.
    manifest(
        root.path(),
        &granted_light(
            "light = { relative = { warmer = 1, dimmer = 1, warm_at_dimmest = true }, for = \"12s\", pattern = { kind = \"breathe\", period = \"6s\" } }",
        ),
    );
    manifest(
        root.path(),
        &granted_light(
            "light = { colour = \"white\", level = 2, pattern = { kind = \"breathe\", period = \"10s\", low = 1 }, for = \"20s\", no_brighter = true }",
        ),
    );
    manifest(
        root.path(),
        &granted_light(
            "rest = { op = \"rest\", during = { from = \"23:00\", to = \"10:00\" }, resettle = { most = 3, after = \"30s\", during = { from = \"23:00\", to = \"06:00\" } } }",
        ),
    );
    for step in [
        "light = { relative = { warmer = 1 }, colour = \"warm\", for = \"20s\" }",
        "light = { relative = { warmer = 0, dimmer = 0 }, for = \"20s\" }",
        "light = { relative = { dimmer = 1 } }",
        "light = { relative = { dimmer = 1 }, for = \"20s\", pattern = { kind = \"breathe\", low = 1 } }",
        "light = { relative = { dimmer = 1 }, for = \"20s\", pattern = { kind = \"candle\" } }",
        "light = { relative = { dimmer = 1, warm_at_dimmest = true }, for = \"20s\" }",
        "light = { relative = { warmer = 1, warm_at_dimmest = false }, for = \"20s\" }",
        "light = { relative = { warmer = 1 }, for = \"20s\", no_brighter = true }",
        "light = { colour = \"white\", for = \"20s\", no_brighter = true }",
        "light = { colour = \"white\", level = 2, keep = true, no_brighter = true }",
        "light = { colour = \"white\", level = 2, for = \"20s\", no_brighter = false }",
        "light = { dim = 1, no_brighter = true }",
        "light = { out = true, no_brighter = true }",
        "rest = { op = \"rest\", during = { from = \"23:00\", to = \"10:00\" }, resettle = { most = 4, after = \"30s\", during = { from = \"23:00\", to = \"06:00\" } } }",
        "rest = { op = \"cancel\", resettle = { most = 1, after = \"30s\", during = { from = \"23:00\", to = \"06:00\" } } }",
    ] {
        fs::write(path.join("skill.toml"), granted_light(step)).unwrap();
        assert!(load(&path).is_err(), "{step}");
    }
}

/// The real bedtime-morning manifest, switched on.
fn bedtime_morning(root: &Path) -> Skill {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/bedtime-morning");
    let folder = root.join("bedtime-morning");
    fs::create_dir_all(&folder).unwrap();
    fs::copy(from.join("SKILL.md"), folder.join("SKILL.md")).unwrap();
    let text = fs::read_to_string(from.join("skill.toml")).unwrap();
    assert!(text.contains("enabled = false"));
    fs::write(
        folder.join("skill.toml"),
        text.replace("enabled = false", "enabled = true"),
    )
    .unwrap();
    load(&folder).unwrap()
}

const FACE: &[u8] =
    br#"{"faces":[{"id":"f1","xmin":0.4,"ymin":0.3,"xmax":0.6,"ymax":0.6,"confidence":0.9}]}"#;
const EMPTY: &[u8] = br#"{"faces":[]}"#;

/// The engine's monotonic clock: microseconds since local midnight.
fn us(minute: u64, second: u64) -> u64 {
    (minute * 60 + second) * 1_000_000
}

/// From `minute`, a minute of the voice resting and `face` (or no one) in
/// view each second: the calls made and the reasons said.
fn morning_minute(engine: &mut Engine, minute: u64, face: bool) -> (Vec<Call>, Vec<String>) {
    let (mut made, mut said) = (Vec::new(), Vec::new());
    for second in 0..60 {
        let now = us(minute, second);
        clock(engine, now, 0, minute);
        engine.observe("voice/state", now, br#"{"state":"resting"}"#, None);
        let runs = engine.observe(Faces::TOPIC, now, if face { FACE } else { EMPTY }, None);
        said.extend(
            runs.iter()
                .flat_map(|run| run.events.iter().map(|event| event.why.clone())),
        );
        made.extend(calls(runs));
    }
    (made, said)
}

fn greeted(made: &[Call]) -> bool {
    made.contains(&Call::Cue)
}

#[test]
fn spoken_to_at_0650_a_face_at_0710_is_not_greeted() {
    // At 05:50, before the window opened, it does not count.
    for (heard_at, greets) in [(6 * 60 + 50, false), (5 * 60 + 50, true)] {
        let root = private_root();
        let mut engine = Engine::new(vec![bedtime_morning(root.path())], true)
            .with_daily_journal(crate::journal(root.path().join("daily.json")));
        assert!(engine.topics().contains(&"voice/heard".into()));
        clock(&mut engine, us(heard_at, 0), 0, heard_at);
        engine.observe(
            "voice/heard",
            us(heard_at, 0),
            br#"{"text":"good morning lamp","finished":true}"#,
            None,
        );
        // Resting a minute, then a face from 07:10.
        morning_minute(&mut engine, 7 * 60 + 9, false);
        let (made, said) = morning_minute(&mut engine, 7 * 60 + 10, true);
        let let_go = said
            .iter()
            .filter(|why| *why == "morning chance let go: spoken to since 06:00")
            .count();
        if greets {
            assert!(greeted(&made), "{made:?} {said:?}");
            assert_eq!(let_go, 0);
        } else {
            assert!(made.is_empty(), "{made:?}");
            assert_eq!(let_go, 1, "said once: {said:?}");
            // The day is spent: a face later that morning is not greeted
            // either, and nothing more is said.
            morning_minute(&mut engine, 7 * 60 + 30, false);
            let (made, said) = morning_minute(&mut engine, 7 * 60 + 31, true);
            assert!(made.is_empty() && said.is_empty(), "{made:?} {said:?}");
        }
    }
}

#[test]
fn two_faces_in_one_morning_greet_once() {
    let root = private_root();
    let mut engine = Engine::new(vec![bedtime_morning(root.path())], true)
        .with_daily_journal(crate::journal(root.path().join("daily.json")));
    morning_minute(&mut engine, 7 * 60 + 9, false);
    let (made, _) = morning_minute(&mut engine, 7 * 60 + 10, true);
    assert!(greeted(&made), "{made:?}");
    assert_eq!(made.iter().filter(|call| **call == Call::Cue).count(), 1);
    // They leave, and a face comes again at 08:00: no second greeting.
    morning_minute(&mut engine, 7 * 60 + 11, false);
    morning_minute(&mut engine, 7 * 60 + 59, false);
    let (made, _) = morning_minute(&mut engine, 8 * 60, true);
    assert!(made.is_empty(), "{made:?}");
}

#[test]
fn unless_heard_goes_with_during() {
    let root = private_root();
    let path = root.path().join("daily-check");
    let text = |trigger: &str| {
        format!(
            "[permissions]\ntopics = [\"clock/local\", \"voice/state\", \"voice/heard\"]\nbody = true\n[trigger]\n{trigger}\nunless_heard = true\n[ask]\npolicy = \"nudge\"\n[[do]]\nbody = {{ gesture = \"stretching\" }}\n"
        )
    };
    let loaded = manifest(root.path(), &text(morning()));
    assert!(loaded.triggers[0].unless_heard);
    fs::write(
        path.join("skill.toml"),
        text("topic = \"clock/local\"\nfield = \"known\"\nequals = true"),
    )
    .unwrap();
    let error = load(&path).unwrap_err();
    assert!(error.contains("unless_heard goes with during"), "{error}");
    // Per item, hearing is never checked, so it is refused (#2664).
    fs::write(
        path.join("skill.toml"),
        text("topic = \"clock/local\"\nduring = { from = \"06:00\", to = \"11:00\" }\neach = \"items\"\nkey = \"id\"\nwhen = \"appears\""),
    )
    .unwrap();
    let error = load(&path).unwrap_err();
    assert!(error.contains("not each"), "{error}");
}

/// Bedtime as on lamp-06c1 (#1672, row 7): an empty desk for 15 minutes,
/// 23:00–06:00, once a day; `daily` false makes it an ordinary trigger.
fn bedtime_watch(root: &Path, daily: bool) -> Engine {
    let text = format!(
        "[[trigger]]\nname = \"bedtime\"\ntopic = \"humans/faces/tracked\"\npresent = false\nfor = \"15m\"\nrearm = \"ends\"\nduring = {{ from = \"23:00\", to = \"06:00\" }}\n{}[ask]\npolicy = \"nudge\"\n[[do]]\non = \"bedtime\"\nbody = {{ gesture = \"stretching\" }}\n",
        if daily { "once_per_day = true\n" } else { "" }
    );
    Engine::new(vec![manifest(root, &text)], true)
        .with_daily_journal(crate::journal(root.join("daily.json")))
}

/// The empty desk seen once a second from 23:10 for `seconds`, and every
/// journal line those messages made.
fn held_empty(engine: &mut Engine, seconds: u64) -> Vec<String> {
    let mut lines = Vec::new();
    for second in 0..=seconds {
        let now = second * 1_000_000;
        clock(engine, now, 0, 23 * 60 + 10 + second / 60);
        lines.extend(reasons(engine.observe(
            Faces::TOPIC,
            now,
            br#"{"faces":[]}"#,
            None,
        )));
    }
    lines
}

fn reasons(runs: Vec<Run>) -> Vec<String> {
    runs.into_iter()
        .flat_map(|run| run.events)
        .map(|event| event.why)
        .collect()
}

fn resets(lines: &[String]) -> Vec<&String> {
    lines
        .iter()
        .filter(|line| line.contains("watch reset"))
        .collect()
}

const A_FACE: &[u8] =
    br#"{"faces":[{"xmin":0.4,"ymin":0.2,"xmax":0.6,"ymax":0.6,"confidence":0.9}]}"#;

#[test]
fn a_daily_watch_reset_by_a_face_says_so_once_with_its_held_time() {
    let root = private_root();
    let mut engine = bedtime_watch(root.path(), true);
    assert!(resets(&held_empty(&mut engine, 400)).is_empty());
    let mut lines = Vec::new();
    for second in 401..=403_u64 {
        let now = second * 1_000_000;
        clock(&mut engine, now, 0, 23 * 60 + 10 + second / 60);
        lines.extend(reasons(engine.observe(Faces::TOPIC, now, A_FACE, None)));
    }
    assert_eq!(
        resets(&lines),
        vec![
            "bedtime watch reset after 400 s: stopped holding: humans/faces/tracked present no longer false"
        ],
        "{lines:?}"
    );
}

#[test]
fn a_daily_watch_leaving_its_window_says_so_once() {
    let root = private_root();
    let mut engine = bedtime_watch(root.path(), true);
    held_empty(&mut engine, 120);
    let mut lines = Vec::new();
    for second in 121..=123_u64 {
        let now = second * 1_000_000;
        // 06:00 the next morning: out of the window.
        clock(&mut engine, now, 1, 6 * 60);
        lines.extend(reasons(engine.observe(
            Faces::TOPIC,
            now,
            br#"{"faces":[]}"#,
            None,
        )));
    }
    assert_eq!(
        resets(&lines),
        vec!["bedtime watch reset after 120 s: left its window 23:00\u{2013}06:00"],
        "{lines:?}"
    );
}

#[test]
fn a_daily_watch_losing_the_trusted_moment_says_so_once() {
    let root = private_root();
    let mut engine = bedtime_watch(root.path(), true);
    held_empty(&mut engine, 120);
    let local = lamp_local_time::LocalTime::Offset(0);
    let mut lines = Vec::new();
    for second in 121..=123_u64 {
        let now = second * 1_000_000;
        engine.clock_in_with_trust(&local, now, MIDNIGHT + (23 * 60 + 12) * 60_000, false);
        lines.extend(reasons(engine.observe(
            Faces::TOPIC,
            now,
            br#"{"faces":[]}"#,
            None,
        )));
    }
    assert_eq!(
        resets(&lines),
        vec!["bedtime watch reset after 120 s: no trusted local moment"],
        "{lines:?}"
    );
}

#[test]
fn a_watch_that_never_held_or_is_not_daily_says_nothing() {
    // Never began holding: a face all along, then out of the window.
    let root = private_root();
    let mut engine = bedtime_watch(root.path(), true);
    let mut lines = Vec::new();
    for second in 0..=10_u64 {
        let now = second * 1_000_000;
        clock(&mut engine, now, 0, 23 * 60 + 10);
        lines.extend(reasons(engine.observe(Faces::TOPIC, now, A_FACE, None)));
    }
    clock(&mut engine, 11_000_000, 1, 7 * 60);
    lines.extend(reasons(engine.observe(
        Faces::TOPIC,
        11_000_000,
        br#"{"faces":[]}"#,
        None,
    )));
    assert!(resets(&lines).is_empty(), "{lines:?}");

    // Not daily: held, then a face, then out of the window.
    let root = private_root();
    let mut engine = bedtime_watch(root.path(), false);
    let mut lines = held_empty(&mut engine, 400);
    clock(&mut engine, 401_000_000, 0, 23 * 60 + 16);
    lines.extend(reasons(engine.observe(
        Faces::TOPIC,
        401_000_000,
        A_FACE,
        None,
    )));
    clock(&mut engine, 402_000_000, 1, 7 * 60);
    lines.extend(reasons(engine.observe(
        Faces::TOPIC,
        402_000_000,
        br#"{"faces":[]}"#,
        None,
    )));
    assert!(resets(&lines).is_empty(), "{lines:?}");
}

#[test]
fn a_daily_watch_reset_by_a_silent_camera_says_so_once() {
    let root = private_root();
    let mut engine = bedtime_watch(root.path(), true);
    held_empty(&mut engine, 400);
    let mut lines = Vec::new();
    // Nothing for 10 s, past the faces' 2 s lifespan, then empty again.
    for second in 410..=412_u64 {
        let now = second * 1_000_000;
        clock(&mut engine, now, 0, 23 * 60 + 17);
        lines.extend(reasons(engine.observe(
            Faces::TOPIC,
            now,
            br#"{"faces":[]}"#,
            None,
        )));
    }
    assert_eq!(
        resets(&lines),
        vec![
            "bedtime watch reset after 400 s: went silent: nothing held it on humans/faces/tracked for 10 s, past its 2 s"
        ],
        "{lines:?}"
    );
}

/// The real welcome-back folder, as installed.
fn welcome_back(root: &Path) -> Skill {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/welcome-back");
    let folder = root.join("welcome-back");
    fs::create_dir_all(folder.join("lights")).unwrap();
    for file in ["SKILL.md", "skill.toml", "lights/warm_glow.toml"] {
        fs::copy(from.join(file), folder.join(file)).unwrap();
    }
    load(&folder).unwrap()
}

/// 2026-01-01, day 0 of [`clock`], in local days since 1970.
const TODAY: i64 = 20_454;

/// From `from` to `to` (minutes), each second the voice's state (`voice`
/// of the second of the minute) and a face from `face_at`: each skill's
/// greetings (runs with calls), in order, and every reason said.
fn desk(
    engine: &mut Engine,
    (from, to): (u64, u64),
    face_at: u64,
    voice: impl Fn(u64) -> &'static [u8],
) -> (Vec<String>, Vec<String>) {
    let (greetings, said) = desk_with(
        engine,
        (from, to),
        |minute, _| if minute >= face_at { FACE } else { EMPTY },
        voice,
    );
    (
        greetings.into_iter().map(|(skill, _)| skill).collect(),
        said,
    )
}

/// As [`desk`], with the faces seen from `face` (of the minute and the
/// second): each greeting with when it came.
fn desk_with(
    engine: &mut Engine,
    (from, to): (u64, u64),
    face: impl Fn(u64, u64) -> &'static [u8],
    voice: impl Fn(u64) -> &'static [u8],
) -> (Vec<(String, u64)>, Vec<String>) {
    let (mut greetings, mut said) = (Vec::new(), Vec::new());
    for minute in from..to {
        for second in 0..60 {
            let now = us(minute, second);
            clock(engine, now, 0, minute);
            engine.observe("voice/state", now, voice(second), None);
            for run in engine.observe(Faces::TOPIC, now, face(minute, second), None) {
                said.extend(
                    run.events
                        .iter()
                        .map(|event| format!("{} {}", run.skill, event.why)),
                );
                if !run.calls.is_empty() && run.calls != [Call::Ends] {
                    greetings.push((run.skill, now));
                }
            }
        }
    }
    (greetings, said)
}

fn resting(_: u64) -> &'static [u8] {
    br#"{"state":"resting"}"#
}

const MORNING: u64 = 7 * 60 + 10;

/// The first builder's ruling on #1672: after a rested night, the morning
/// greeting is the welcome back. Either skill first in the folder order.
#[test]
fn a_rested_night_and_a_first_face_greet_once_with_the_morning() {
    for bedtime_first in [true, false] {
        let root = private_root();
        let (bedtime, welcome) = (bedtime_morning(root.path()), welcome_back(root.path()));
        let skills = if bedtime_first {
            vec![bedtime, welcome]
        } else {
            vec![welcome, bedtime]
        };
        let mut engine = Engine::new(skills, true)
            .with_daily_journal(crate::journal(root.path().join("daily.json")));
        engine.rested(Some(TODAY - 1));
        let (greetings, said) = desk(&mut engine, (MORNING - 6, MORNING + 3), MORNING, resting);
        assert_eq!(greetings, vec!["bedtime-morning"], "{said:?}");
        assert!(
            said.contains(
                &"welcome-back done: bedtime-morning's morning greeting was this face's hello"
                    .into()
            ),
            "{said:?}"
        );
        // Away past welcome-back's five minutes and back at 08:00: the
        // morning is spent, so welcome-back greets as it always did.
        let (greetings, said) = desk(&mut engine, (MORNING + 3, 8 * 60 + 1), 8 * 60, resting);
        assert_eq!(greetings, vec!["welcome-back"], "{said:?}");
    }
}

/// The morning's conditions never hold within its minute's wait (the voice
/// hears something every 20 s): its chance is dropped, and welcome-back,
/// which waited, greets once after the drop.
#[test]
fn a_dropped_morning_lets_welcome_back_greet_after_it() {
    let root = private_root();
    let mut engine = Engine::new(
        vec![bedtime_morning(root.path()), welcome_back(root.path())],
        true,
    )
    .with_daily_journal(crate::journal(root.path().join("daily.json")));
    engine.rested(Some(TODAY - 1));
    let voice = |second: u64| -> &'static [u8] {
        if second.is_multiple_of(20) {
            br#"{"state":"listening"}"#
        } else {
            br#"{"state":"resting"}"#
        }
    };
    let (greetings, said) = desk(&mut engine, (MORNING - 6, MORNING + 3), MORNING, voice);
    assert_eq!(greetings, vec!["welcome-back"], "{said:?}");
    let waits = said
        .iter()
        .position(|why| why == "welcome-back waits: the morning greeting comes first")
        .expect("said it waits");
    let dropped = said
        .iter()
        .position(|why| why.starts_with("bedtime-morning morning chance dropped"))
        .expect("the morning was dropped");
    assert!(waits < dropped, "{said:?}");
    assert_eq!(
        said.iter().filter(|why| why.contains("waits")).count(),
        1,
        "said once: {said:?}"
    );
}

/// No rest that night, or the night before last: welcome-back greets as it
/// does today, beside the morning, with no wait.
#[test]
fn without_a_rested_night_welcome_back_does_not_wait() {
    for rested in [None, Some(TODAY - 2), Some(TODAY)] {
        let root = private_root();
        let mut engine = Engine::new(
            vec![bedtime_morning(root.path()), welcome_back(root.path())],
            true,
        )
        .with_daily_journal(crate::journal(root.path().join("daily.json")));
        engine.rested(rested);
        let (greetings, said) = desk(&mut engine, (MORNING - 6, MORNING + 3), MORNING, resting);
        assert!(
            greetings.contains(&"welcome-back".to_owned()),
            "{rested:?} {said:?}"
        );
        assert!(!said.iter().any(|why| why.contains("waits")), "{said:?}");
    }
}

/// Bedtime and Morning not loaded (disabled, or a routine with
/// `LAMP_SKILL_ROUTINES` off): welcome-back is as today, and the journal
/// says its `yields_to` is ignored.
#[test]
fn without_bedtime_morning_welcome_back_does_not_wait() {
    let root = private_root();
    let welcome = welcome_back(root.path());
    let notes = lamp_skills::yield_notes(std::slice::from_ref(&welcome));
    assert_eq!(
        notes,
        vec![
            "welcome-back: yields_to bedtime-morning's morning, which is not a loaded daily trigger: ignored, it does not wait"
        ]
    );
    let bedtime = bedtime_morning(root.path());
    assert!(lamp_skills::yield_notes(&[bedtime, welcome.clone()]).is_empty());
    let mut engine = Engine::new(vec![welcome], true)
        .with_daily_journal(crate::journal(root.path().join("daily.json")));
    engine.rested(Some(TODAY - 1));
    let (greetings, said) = desk(&mut engine, (MORNING - 6, MORNING + 3), MORNING, resting);
    assert_eq!(greetings, vec!["welcome-back"], "{said:?}");
    assert!(!said.iter().any(|why| why.contains("waits")), "{said:?}");
}

#[test]
fn yields_to_goes_with_an_appears_trigger() {
    let root = private_root();
    let path = root.path().join("daily-check");
    manifest(
        root.path(),
        "[trigger]\ntopic = \"humans/faces/tracked\"\nwhen = \"appears\"\nyields_to = { skill = \"bedtime-morning\", trigger = \"morning\" }\n[ask]\npolicy = \"hello\"\n[[do]]\nbody = { gesture = \"stretching\" }\n",
    );
    for trigger in [
        "topic = \"humans/faces/tracked\"\npresent = true\nyields_to = { skill = \"bedtime-morning\", trigger = \"morning\" }",
        "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\nyields_to = { skill = \"Bedtime\", trigger = \"morning\" }",
        "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\nyields_to = { skill = \"bedtime-morning\", trigger = \"the-morning\" }",
        "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\nyields_to = { skill = \"bedtime-morning\" }",
    ] {
        fs::write(
            path.join("skill.toml"),
            granted(&format!(
                "[trigger]\n{trigger}\n[ask]\npolicy = \"hello\"\n[[do]]\nbody = {{ gesture = \"stretching\" }}\n"
            )),
        )
        .unwrap();
        let error = load(&path).unwrap_err();
        assert!(
            error.contains("yields_to") || error.contains("missing field `trigger`"),
            "{trigger}: {error}"
        );
    }
}

/// A face at the desk a little farther than the morning's `near`, and one
/// that looks down at the keyboard every other second (the reviewer's P2 on
/// #2757). Welcome-back's own `near` is the morning's, so the far face is
/// greeted by neither and waited for by nothing. The glancing one holds
/// welcome-back through its 3 s `gap` but never holds the morning its 3 s:
/// the morning is never judged, and welcome-back greets it once at its
/// own 3 s, not at 11:00, and the morning never fires.
#[test]
fn a_face_the_morning_never_holds_for_is_greeted_by_welcome_back_at_once() {
    const FAR: &[u8] =
        br#"{"faces":[{"id":"f1","xmin":0.45,"ymin":0.4,"xmax":0.55,"ymax":0.52,"confidence":0.9}]}"#;
    for bedtime_first in [true, false] {
        let root = private_root();
        let (bedtime, welcome) = (bedtime_morning(root.path()), welcome_back(root.path()));
        let skills = if bedtime_first {
            vec![bedtime, welcome]
        } else {
            vec![welcome, bedtime]
        };
        let mut engine = Engine::new(skills, true)
            .with_daily_journal(crate::journal(root.path().join("daily.json")));
        engine.rested(Some(TODAY - 1));
        let far = |minute, _| if minute >= MORNING { FAR } else { EMPTY };
        let (greetings, said) = desk_with(&mut engine, (MORNING - 6, MORNING + 3), far, resting);
        assert!(greetings.is_empty(), "{said:?}");
        assert!(!said.iter().any(|why| why.contains("waits")), "{said:?}");
        let glancing = |minute, second: u64| {
            if minute >= MORNING + 3 && !second.is_multiple_of(2) {
                FACE
            } else {
                EMPTY
            }
        };
        let (greetings, said) = desk_with(&mut engine, (MORNING + 3, 11 * 60), glancing, resting);
        let names: Vec<_> = greetings.iter().map(|(skill, _)| skill.as_str()).collect();
        assert_eq!(names, ["welcome-back"], "{said:?}");
        assert!(
            greetings[0].1 <= us(MORNING + 3, 10),
            "greeted at {} s: {said:?}",
            greetings[0].1 / 1_000_000
        );
    }
}

/// The first builder on #2757: once welcome-back greeted someone in the
/// morning's window after a rested night, that was the day's morning
/// moment. Its hello after a dropped morning claims the morning's day in
/// the journal, said once, so a second face at 09:00 is given
/// welcome-back's ordinary hello, never the morning's.
#[test]
fn welcome_backs_hello_after_a_dropped_morning_claims_the_morning() {
    let root = private_root();
    let path = root.path().join("daily.json");
    let mut engine = Engine::new(
        vec![bedtime_morning(root.path()), welcome_back(root.path())],
        true,
    )
    .with_daily_journal(crate::journal(&path));
    engine.rested(Some(TODAY - 1));
    let voice = |second: u64| -> &'static [u8] {
        if second.is_multiple_of(20) {
            br#"{"state":"listening"}"#
        } else {
            br#"{"state":"resting"}"#
        }
    };
    let (greetings, said) = desk(&mut engine, (MORNING - 6, MORNING + 3), MORNING, voice);
    assert_eq!(greetings, vec!["welcome-back"], "{said:?}");
    let let_go = "bedtime-morning morning chance let go: welcome-back already greeted this morning";
    assert_eq!(
        said.iter().filter(|why| *why == let_go).count(),
        1,
        "{said:?}"
    );
    // Away past welcome-back's five minutes and its cooldown, back at 09:00
    // with the room quiet: welcome-back's hello, and no morning step.
    let (greetings, said) = desk(&mut engine, (MORNING + 3, 9 * 60 + 1), 9 * 60, resting);
    assert_eq!(greetings, vec!["welcome-back"], "{said:?}");
    assert!(!said.iter().any(|why| why == let_go), "{said:?}");
    // The claim is the journal's, on disk, as the morning's own would be.
    drop(engine);
    assert!(crate::journal(&path).claimed("bedtime-morning", "morning", TODAY));
}

/// The restart question on #2757: lamp-skills restarts after the morning's
/// hum. The fresh Engine reads the same journal, with the morning's day
/// claimed, and the night rested as the rest manager reads it back
/// (`routine::RESTED_FILE`). The face there at the start is no news to
/// welcome-back's `appears`, and one back two minutes later has not been
/// away its five: neither skill greets again.
#[test]
fn a_restart_after_the_morning_greets_nobody_twice() {
    for face_at in [MORNING + 3, MORNING + 5] {
        let root = private_root();
        let path = root.path().join("daily.json");
        let mut engine = Engine::new(
            vec![bedtime_morning(root.path()), welcome_back(root.path())],
            true,
        )
        .with_daily_journal(crate::journal(&path));
        engine.rested(Some(TODAY - 1));
        let (greetings, said) = desk(&mut engine, (MORNING - 6, MORNING + 2), MORNING, resting);
        assert_eq!(greetings, vec!["bedtime-morning"], "{said:?}");
        drop(engine);
        let mut engine = Engine::new(
            vec![bedtime_morning(root.path()), welcome_back(root.path())],
            true,
        )
        .with_daily_journal(crate::journal(&path));
        engine.rested(Some(TODAY - 1));
        let (greetings, said) = desk(&mut engine, (MORNING + 3, MORNING + 30), face_at, resting);
        assert!(greetings.is_empty(), "face at {face_at}: {said:?}");
    }
}

/// A restart before the morning fires: the night rested read back, the
/// first face gets the morning's greeting, once, and welcome-back yields.
#[test]
fn a_restart_before_the_morning_still_greets_with_it() {
    let root = private_root();
    let path = root.path().join("daily.json");
    let engine = Engine::new(
        vec![bedtime_morning(root.path()), welcome_back(root.path())],
        true,
    )
    .with_daily_journal(crate::journal(&path));
    drop(engine);
    let mut engine = Engine::new(
        vec![welcome_back(root.path()), bedtime_morning(root.path())],
        true,
    )
    .with_daily_journal(crate::journal(&path));
    engine.rested(Some(TODAY - 1));
    let (greetings, said) = desk(&mut engine, (MORNING - 6, MORNING + 3), MORNING, resting);
    assert_eq!(greetings, vec!["bedtime-morning"], "{said:?}");
}

/// #2602: each due chance Welcome back drops says one journal line naming
/// its gate: an absence under its 300 s (once through a face that flickers
/// past its gap) and its cooldown. Its 60 s wait for the voice running out
/// is #3007's `chance dropped` line, not one of these: said once.
#[test]
fn welcome_back_says_each_dropped_chance_once_with_its_gate() {
    let root = private_root();
    let mut engine = Engine::new(vec![welcome_back(root.path())], true);
    let face = |second: u64| {
        [
            (0, 5),
            (220, 225),
            (230, 235),
            (600, 605),
            (1000, 1005),
            (2400, 2470),
        ]
        .iter()
        .any(|(from, to)| (*from..=*to).contains(&second))
    };
    let mut greeted = Vec::new();
    for second in 0..=2470 {
        let now = second * 1_000_000;
        let voice: &[u8] = if second >= 2380 {
            br#"{"state":"speaking"}"#
        } else {
            br#"{"state":"resting"}"#
        };
        engine.observe("voice/state", now, voice, None);
        let runs = engine.observe(
            Faces::TOPIC,
            now,
            if face(second) { FACE } else { EMPTY },
            None,
        );
        if runs.iter().any(|run| !run.calls.is_empty()) {
            greeted.push(second);
        }
    }
    assert_eq!(greeted, [603]);
    assert_eq!(
        engine.take_dropped(),
        [
            "welcome-back not run: away 0 s, under its 300 s",
            "welcome-back not run: away 215 s, under its 300 s",
            "welcome-back not run: in its cooldown, 14 min left",
        ]
    );
}

/// #2825, lamp-52e6 at 02:37: a face greeted at its arrival, gone for
/// 20 min, then back with a glimpse of 2 s lost past its gap before it
/// settles. The glimpse is shorter than `for`, so it is no visit: the
/// away clock still runs from when they left, and they are greeted once.
/// A look away past the gap within one visit is no leaving: no hello again.
#[test]
fn welcome_back_counts_away_from_the_last_visit_not_a_glimpse() {
    let root = private_root();
    let mut engine = Engine::new(vec![welcome_back(root.path())], true);
    let face = |second: u64| {
        [(600, 899), (906, 1199), (2400, 2401), (2406, 2600)]
            .iter()
            .any(|(from, to)| (*from..=*to).contains(&second))
    };
    let mut greeted = Vec::new();
    for second in 0..=2600 {
        let now = second * 1_000_000;
        engine.observe("voice/state", now, br#"{"state":"resting"}"#, None);
        let runs = engine.observe(
            Faces::TOPIC,
            now,
            if face(second) { FACE } else { EMPTY },
            None,
        );
        if runs.iter().any(|run| !run.calls.is_empty()) {
            greeted.push(second);
        }
    }
    assert_eq!(greeted, [603, 2409]);
    assert_eq!(
        engine.take_dropped(),
        ["welcome-back not run: away 7 s, under its 300 s"]
    );
}

/// #2602: a daily trigger's day already claimed, and a trigger spoken to
/// since its window opened, each say why their chance was not run.
#[test]
fn a_claimed_day_and_a_spoken_window_say_why_not_run() {
    let root = private_root();
    let mut engine = Engine::new(vec![skill(root.path(), "", morning())], true)
        .with_daily_journal(crate::journal(root.path().join("daily.json")));
    clock(&mut engine, 0, 0, 360);
    assert_eq!(calls(engine.clock_tick(0)).len(), 1);
    clock(&mut engine, 1, 0, 300);
    engine.clock_tick(1);
    clock(&mut engine, 2, 0, 361);
    assert!(calls(engine.clock_tick(2)).is_empty());
    assert_eq!(
        engine.take_dropped(),
        ["daily-check not run: its day already claimed"]
    );
    let root = private_root();
    let heard = manifest(
        root.path(),
        "[permissions]\ntopics = [\"clock/local\", \"voice/heard\"]\nbody = true\n[trigger]\ntopic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\nduring = { from = \"06:00\", to = \"10:00\" }\nunless_heard = true\n[ask]\npolicy = \"nudge\"\n[[do]]\nbody = { gesture = \"stretching\" }\n",
    );
    let mut engine = Engine::new(vec![heard], true);
    clock(&mut engine, us(390, 0), 0, 390);
    engine.observe(
        "voice/heard",
        us(390, 0),
        br#"{"text":"fixture words","finished":true}"#,
        None,
    );
    clock(&mut engine, us(400, 0), 0, 400);
    assert!(calls(engine.clock_tick(us(400, 0))).is_empty());
    assert_eq!(
        engine.take_dropped(),
        ["daily-check not run: spoken to since 06:00"]
    );
}
