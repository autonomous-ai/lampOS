use lamp_apps::layout::Layout;
use lamp_apps::remove::{NOT_INSTALLED, Removed, remove};
use lamp_apps::state::{Mark, has, set};
use lamp_apps::testing::{FakeCompanions, FakeEffects};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// An installed app as systemd leaves it: code with a `current` link, data
/// at the private path behind the `/var/lib/lampOS/apps/<name>` link.
fn installed(layout: &Layout, name: &str) {
    write(&layout.code.join(name).join("1.0.0/src/app.py"), "x");
    symlink("1.0.0", layout.code.join(name).join("current")).unwrap();
    write(&layout.data_real(name).join("notes.txt"), "data");
    fs::create_dir_all(&layout.data).unwrap();
    symlink(layout.data_real(name), layout.data_link(name)).unwrap();
}

fn rig() -> (tempfile::TempDir, Layout) {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    (root, layout)
}

fn gone(layout: &Layout, name: &str) {
    assert!(!layout.code.join(name).exists(), "code");
    assert!(layout.data_link(name).symlink_metadata().is_err(), "link");
    assert!(layout.data_real(name).symlink_metadata().is_err(), "data");
    for mark in [Mark::Installing, Mark::Removing, Mark::Stopped] {
        assert!(!has(&layout.state, name, mark), "{mark:?}");
    }
}

#[test]
fn remove_runs_in_one_order_and_leaves_nothing() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    set(&layout.state, "x", Mark::Stopped).unwrap();
    let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "x"),
        Ok(Removed::Done)
    );
    assert_eq!(
        fx.calls,
        [
            "/usr/bin/systemctl stop -- lampapp@x.service",
            "/usr/bin/systemctl stop -- lampapp-net@x.service",
            "empty? lampapp@x.service",
            "empty? lampapp-net@x.service",
            "/usr/bin/systemctl reset-failed -- lampapp@x.service",
            "/usr/bin/systemctl reset-failed -- lampapp-net@x.service",
            "/usr/bin/systemctl clean --what=state -- lampapp@x.service",
            "/usr/bin/systemctl clean --what=state -- lampapp-net@x.service",
            "app_forget x",
        ]
    );
    assert_eq!(skills.removed, ["x"]);
    gone(&layout, "x");
}

/// lamp-control keeps an app's settings, notes, clips and moves, and drops
/// them on `app_forget`. It is said only once both unit variants have stopped
/// and are empty (a running app could set them again), and a remove it does
/// not answer is cut, not done.
#[test]
fn lamp_control_is_told_only_after_both_units_have_stopped() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    let mut fx = FakeEffects {
        never_empty: true,
        ..FakeEffects::cleaning(&layout)
    };
    let mut skills = FakeCompanions::default();
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert!(
        !fx.calls.iter().any(|call| call.starts_with("app_forget")),
        "a unit still has a process: {:?}",
        fx.calls
    );
    let mut fx = FakeEffects::cleaning(&layout);
    remove(&layout, &mut fx, &mut skills, "x").unwrap();
    let at = |what: &str| fx.calls.iter().position(|c| c.contains(what)).unwrap();
    assert!(at("empty? lampapp@x") < at("app_forget x"));
    assert!(at("empty? lampapp-net@x") < at("app_forget x"));
    assert!(at("stop -- lampapp-net@x") < at("app_forget x"));
    assert_eq!(
        fx.calls
            .iter()
            .filter(|c| c.starts_with("app_forget"))
            .count(),
        1
    );
}

#[test]
fn a_forget_lamp_control_does_not_answer_leaves_the_remove_cut_and_the_next_one_finishes() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    let mut skills = FakeCompanions::default();
    // The forget is the ninth call of a whole remove.
    let mut fx = FakeEffects {
        fail_at: Some(8),
        ..FakeEffects::cleaning(&layout)
    };
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert_eq!(fx.calls.last().map(String::as_str), Some("app_forget x"));
    assert!(layout.code.join("x").exists(), "the code stays");
    assert!(has(&layout.state, "x", Mark::Removing), "the mark stays");
    assert!(skills.removed.is_empty(), "no companion skill went yet");
    // Asked again, with no second yes in the way, it is told once more and ends.
    let mut fx = FakeEffects::cleaning(&layout);
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "x"),
        Ok(Removed::Done)
    );
    assert!(fx.calls.iter().any(|c| c == "app_forget x"));
    gone(&layout, "x");
}

#[test]
fn the_removing_mark_is_written_before_anything_is_touched() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    let mut fx = FakeEffects {
        fail_at: Some(0),
        ..FakeEffects::cleaning(&layout)
    };
    let mut skills = FakeCompanions::default();
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert!(has(&layout.state, "x", Mark::Removing));
    assert!(layout.data_real("x/notes.txt").exists(), "data untouched");
    assert!(
        layout.code.join("x/1.0.0/src/app.py").exists(),
        "code untouched"
    );
    assert!(skills.removed.is_empty());
}

#[test]
fn nothing_is_deleted_while_the_unit_still_has_a_process() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    let mut fx = FakeEffects {
        never_empty: true,
        ..FakeEffects::cleaning(&layout)
    };
    let mut skills = FakeCompanions::default();
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert!(layout.data_real("x/notes.txt").exists());
    assert!(layout.code.join("x").exists());
    assert!(has(&layout.state, "x", Mark::Removing));
    // A slow stop that does empty in time still removes.
    let mut fx = FakeEffects {
        busy_polls: 3,
        ..FakeEffects::cleaning(&layout)
    };
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "x"),
        Ok(Removed::Done)
    );
    gone(&layout, "x");
}

#[test]
fn an_app_that_is_not_there_is_a_clean_no_with_no_effect() {
    let (_root, layout) = rig();
    let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "x"),
        Err(NOT_INSTALLED.to_owned())
    );
    assert!(fx.calls.is_empty());
    assert!(skills.removed.is_empty());
    assert!(!has(&layout.state, "x", Mark::Removing));
}

#[test]
fn a_bad_name_is_refused_before_anything() {
    let (_root, layout) = rig();
    let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
    for bad in ["", "../x", "X", "a/b", "9"] {
        assert!(
            remove(&layout, &mut fx, &mut skills, bad).is_err(),
            "{bad:?}"
        );
    }
    assert!(fx.calls.is_empty());
}

#[test]
fn a_code_folder_that_is_a_link_is_unlinked_not_followed() {
    let (root, layout) = rig();
    let target = root.path().join("target");
    write(&target.join("keep"), "k");
    fs::create_dir_all(&layout.code).unwrap();
    symlink(&target, layout.code.join("x")).unwrap();
    set(&layout.state, "x", Mark::Removing).unwrap();
    let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "x"),
        Ok(Removed::Done)
    );
    assert!(target.join("keep").exists());
    assert!(layout.code.join("x").symlink_metadata().is_err());
}

#[test]
fn a_crash_at_any_step_then_a_retry_ends_clean() {
    // Every systemctl call and the companions' removal fails once; the retry
    // (the sweep's job, or the owner asking again) finishes the job.
    for at in 0..8 {
        let (_root, layout) = rig();
        installed(&layout, "x");
        set(&layout.state, "x", Mark::Stopped).unwrap();
        let mut fx = FakeEffects {
            fail_at: Some(at),
            ..FakeEffects::cleaning(&layout)
        };
        let mut skills = FakeCompanions::default();
        let first = remove(&layout, &mut fx, &mut skills, "x");
        if first.is_ok() {
            gone(&layout, "x");
            continue;
        }
        assert!(has(&layout.state, "x", Mark::Removing), "call {at}");
        let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
        assert_eq!(
            remove(&layout, &mut fx, &mut skills, "x"),
            Ok(Removed::Done),
            "call {at}"
        );
        gone(&layout, "x");
    }
    let (_root, layout) = rig();
    installed(&layout, "x");
    let mut fx = FakeEffects::cleaning(&layout);
    let mut skills = FakeCompanions {
        fail: true,
        ..Default::default()
    };
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert!(has(&layout.state, "x", Mark::Removing));
    let mut skills = FakeCompanions::default();
    assert_eq!(
        remove(
            &layout,
            &mut FakeEffects::cleaning(&layout),
            &mut skills,
            "x"
        ),
        Ok(Removed::Done)
    );
    gone(&layout, "x");
}

#[test]
fn argv_is_absolute_with_the_double_dash_before_the_name() {
    use lamp_apps::effects::{Verb, argv};
    assert_eq!(
        argv(Verb::Stop, "lampapp@x.service"),
        ["/usr/bin/systemctl", "stop", "--", "lampapp@x.service"]
    );
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/effects.rs")).unwrap();
    assert!(source.contains(".env_clear()"), "a cleared environment");
}

#[test]
fn a_unit_that_is_already_gone_is_done_not_an_error() {
    use lamp_apps::effects::{Verb, judge_status};
    // 5 is systemctl's "not loaded".
    for verb in [Verb::Stop, Verb::ResetFailed] {
        assert_eq!(judge_status(verb, "lampapp@x.service", Some(5)), Ok(()));
    }
    for verb in [Verb::Start, Verb::Restart] {
        assert!(judge_status(verb, "lampapp@x.service", Some(5)).is_err());
    }
    assert!(judge_status(Verb::Stop, "u", Some(1)).is_err());
    assert!(judge_status(Verb::Stop, "u", None).is_err());
    assert_eq!(judge_status(Verb::Stop, "u", Some(0)), Ok(()));
}

/// 0c89, 18 Nov: lampapp@store-test had exited and unloaded, `reset-failed`
/// said "not loaded", and the remove stopped halfway.
#[test]
fn a_remove_of_an_app_whose_units_are_already_unloaded_runs_to_the_end() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    set(&layout.state, "x", Mark::Stopped).unwrap();
    let mut fx = FakeEffects::cleaning(&layout);
    for unit in ["lampapp@x.service", "lampapp-net@x.service"] {
        for verb in ["stop", "reset-failed"] {
            fx.exits
                .push((format!("/usr/bin/systemctl {verb} -- {unit}"), 5));
        }
    }
    let mut skills = FakeCompanions::default();
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "x"),
        Ok(Removed::Done)
    );
    gone(&layout, "x");
    assert_eq!(skills.removed, ["x"]);
}

/// A remove cut after its marks and before its code: the next start-up's
/// sweep finishes it (and so does the owner asking again, with no second yes).
#[test]
fn a_cut_remove_is_finished_by_the_next_start_up_sweep() {
    use lamp_apps::sweep::sweep;
    let (_root, layout) = rig();
    installed(&layout, "x");
    let mut cut = FakeEffects::cleaning(&layout);
    cut.exits.push((
        "/usr/bin/systemctl reset-failed -- lampapp@x.service".into(),
        1,
    ));
    let mut skills = FakeCompanions::default();
    assert!(remove(&layout, &mut cut, &mut skills, "x").is_err());
    assert!(has(&layout.state, "x", Mark::Removing));
    assert!(
        layout.code.join("x/1.0.0").exists(),
        "the code is still there"
    );
    let mut fx = FakeEffects::cleaning(&layout);
    let report = sweep(&layout, &mut fx, &mut skills);
    assert_eq!(report.resumed_removes, ["x"]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    gone(&layout, "x");
}

#[test]
fn the_data_is_systemds_to_remove_and_the_helper_deletes_none_of_it() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    // systemd (the fake does not clean) leaves the data: the remove stops
    // before the code goes, with its mark, and deletes nothing itself.
    let mut fx = FakeEffects::default();
    let mut skills = FakeCompanions::default();
    let error = remove(&layout, &mut fx, &mut skills, "x").unwrap_err();
    assert!(
        error.contains("still there after systemctl clean"),
        "{error}"
    );
    assert!(layout.data_real("x/notes.txt").exists());
    assert!(layout.data_link("x").symlink_metadata().is_ok());
    assert!(layout.code.join("x").exists());
    assert!(has(&layout.state, "x", Mark::Removing));
    assert!(skills.removed.is_empty());
    // A retry once systemd has cleaned finishes.
    assert_eq!(
        remove(
            &layout,
            &mut FakeEffects::cleaning(&layout),
            &mut skills,
            "x"
        ),
        Ok(Removed::Done)
    );
    gone(&layout, "x");
}

#[test]
fn the_clean_line_is_state_only_and_the_source_deletes_no_data() {
    use lamp_apps::effects::{Verb, argv};
    assert_eq!(
        argv(Verb::Clean, "lampapp@x.service"),
        [
            "/usr/bin/systemctl",
            "clean",
            "--what=state",
            "--",
            "lampapp@x.service"
        ]
    );
    // The remove names no data path to delete: it never calls remove_dir_all
    // or remove_file on `data_real` or `data_link`.
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/remove.rs")).unwrap();
    assert!(!source.contains("remove_entry(&real)"));
    assert!(!source.contains("data_real(name)).map_err"));
}

/// `systemctl clean` is systemd's to do; if it ran and the data is still
/// there the remove stops at its mark and deletes nothing itself.
#[test]
fn data_that_survives_the_clean_stops_the_remove_and_is_not_deleted_by_it() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    // A fake whose `clean` removes nothing.
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    let said = remove(&layout, &mut fx, &mut skills, "x").unwrap_err();
    assert!(said.contains("still there after systemctl clean"), "{said}");
    assert!(
        layout.data_real("x").join("notes.txt").exists(),
        "not deleted by the helper"
    );
    assert!(layout.code.join("x").exists());
    assert!(has(&layout.state, "x", Mark::Removing));
    assert!(
        !fx.calls.iter().any(|c| c.starts_with("app_forget")),
        "no forget before the data is gone"
    );
}
