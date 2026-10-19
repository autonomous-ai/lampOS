//! Local windows and durable daily chances, through the real skill loader
//! and engine. No lamp or wall-clock wait is needed.
use lamp_msgs::Faces;
use lamp_skills::{Call, Engine, LOCAL_CLOCK_TOPIC, Run, Skill, daily::Journal, load, load_all};
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
    assert_eq!(skill.triggers.len(), 3);
    assert!(skill.triggers.iter().all(|trigger| trigger.once_per_day));
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
        .with_daily_journal(Journal::open(root.path().join("daily.json")).unwrap());
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
    let journal = Journal::open(root.path().join("daily.json")).unwrap();
    let mut engine = Engine::new(vec![loaded], true).with_daily_journal(journal);
    clock(&mut engine, 0, 0, 360);
    assert_eq!(calls(engine.clock_tick(0)).len(), 1);
}

#[test]
fn a_consumed_morning_survives_restart_reload_and_a_backward_day() {
    let root = private_root();
    let loaded = skill(root.path(), "", morning());
    let path = root.path().join("daily.json");
    {
        let mut engine = Engine::new(vec![loaded.clone()], true)
            .with_daily_journal(Journal::open(&path).unwrap());
        clock(&mut engine, 0, 2, 360);
        assert_eq!(calls(engine.clock_tick(0)).len(), 1);
        assert!(engine.replace(vec![loaded.clone()]).is_empty());
        assert!(calls(engine.clock_tick(1)).is_empty());
    }
    let mut engine =
        Engine::new(vec![loaded], true).with_daily_journal(Journal::open(&path).unwrap());
    clock(&mut engine, 0, 2, 370);
    assert!(
        calls(engine.clock_tick(0)).is_empty(),
        "restart in the same morning"
    );
    clock(&mut engine, 1, 1, 300);
    engine.clock_tick(1);
    clock(&mut engine, 2, 1, 360);
    assert!(
        calls(engine.clock_tick(2)).is_empty(),
        "clock rolled back a day"
    );
    clock(&mut engine, 3, 3, 300);
    engine.clock_tick(3);
    clock(&mut engine, 4, 3, 360);
    assert_eq!(calls(engine.clock_tick(4)).len(), 1);
}

#[test]
fn the_daily_claim_precedes_a_policy_answer_and_is_not_replayed_after_refusal() {
    let root = private_root();
    let loaded = skill(root.path(), "", morning());
    let path = root.path().join("daily.json");
    let run = {
        let mut engine = Engine::new(vec![loaded.clone()], false)
            .asking_the_policy()
            .with_daily_journal(Journal::open(&path).unwrap());
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
    let mut restarted =
        Engine::new(vec![loaded], true).with_daily_journal(Journal::open(path).unwrap());
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
        .with_daily_journal(Journal::open(root.path().join("daily.json")).unwrap());
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
    assert!(engine.clock_tick(2).is_empty());
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
    engine = engine.with_daily_journal(Journal::open(root.path().join("daily.json")).unwrap());
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
        .with_daily_journal(Journal::open(root.path().join("daily.json")).unwrap());
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
