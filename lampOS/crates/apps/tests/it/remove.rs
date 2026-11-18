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
    write(&layout.private.join(name).join("notes.txt"), "data");
    fs::create_dir_all(&layout.data).unwrap();
    symlink(layout.private.join(name), layout.data.join(name)).unwrap();
}

fn rig() -> (tempfile::TempDir, Layout) {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    (root, layout)
}

fn gone(layout: &Layout, name: &str) {
    assert!(!layout.code.join(name).exists(), "code");
    assert!(layout.data.join(name).symlink_metadata().is_err(), "link");
    assert!(
        layout.private.join(name).symlink_metadata().is_err(),
        "data"
    );
    for mark in [Mark::Installing, Mark::Removing, Mark::Stopped] {
        assert!(!has(&layout.state, name, mark), "{mark:?}");
    }
}

#[test]
fn remove_runs_in_one_order_and_leaves_nothing() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    set(&layout.state, "x", Mark::Stopped).unwrap();
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
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
        ]
    );
    assert_eq!(skills.removed, ["x"]);
    gone(&layout, "x");
}

#[test]
fn the_removing_mark_is_written_before_anything_is_touched() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    let mut fx = FakeEffects {
        fail_at: Some(0),
        ..Default::default()
    };
    let mut skills = FakeCompanions::default();
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert!(has(&layout.state, "x", Mark::Removing));
    assert!(
        layout.private.join("x/notes.txt").exists(),
        "data untouched"
    );
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
        ..Default::default()
    };
    let mut skills = FakeCompanions::default();
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert!(layout.private.join("x/notes.txt").exists());
    assert!(layout.code.join("x").exists());
    assert!(has(&layout.state, "x", Mark::Removing));
    // A slow stop that does empty in time still removes.
    let mut fx = FakeEffects {
        busy_polls: 3,
        ..Default::default()
    };
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "x"),
        Ok(Removed::Done)
    );
    gone(&layout, "x");
}

#[test]
fn a_link_and_a_hard_link_planted_in_the_data_folder_cost_nothing_outside() {
    let (root, layout) = rig();
    installed(&layout, "x");
    let outside = root.path().join("outside");
    fs::create_dir_all(outside.join("dir")).unwrap();
    fs::write(outside.join("dir/keep.txt"), "keep").unwrap();
    fs::write(outside.join("file.txt"), "keep").unwrap();
    let data = layout.private.join("x");
    symlink(outside.join("dir"), data.join("to-dir")).unwrap();
    symlink(outside.join("file.txt"), data.join("to-file")).unwrap();
    fs::hard_link(outside.join("file.txt"), data.join("hard")).unwrap();
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "x"),
        Ok(Removed::Done)
    );
    assert_eq!(
        fs::read_to_string(outside.join("dir/keep.txt")).unwrap(),
        "keep"
    );
    assert_eq!(
        fs::read_to_string(outside.join("file.txt")).unwrap(),
        "keep"
    );
    gone(&layout, "x");
}

#[test]
fn a_data_link_that_points_elsewhere_is_left_alone_with_its_target() {
    let (root, layout) = rig();
    installed(&layout, "x");
    let elsewhere = root.path().join("elsewhere");
    write(&elsewhere.join("precious"), "p");
    fs::remove_file(layout.data.join("x")).unwrap();
    symlink(&elsewhere, layout.data.join("x")).unwrap();
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    let error = remove(&layout, &mut fx, &mut skills, "x").unwrap_err();
    assert!(
        error.contains("elsewhere") || error.contains("points"),
        "{error}"
    );
    assert!(elsewhere.join("precious").exists());
    assert!(
        layout.data.join("x").symlink_metadata().is_ok(),
        "the link stays"
    );
    assert!(
        layout.code.join("x").exists(),
        "the code is left, the remove is unfinished"
    );
    assert!(has(&layout.state, "x", Mark::Removing));
}

#[test]
fn a_real_folder_where_the_link_should_be_is_refused() {
    let (_root, layout) = rig();
    installed(&layout, "x");
    fs::remove_file(layout.data.join("x")).unwrap();
    write(&layout.data.join("x").join("f"), "f");
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert!(layout.data.join("x/f").exists());
}

#[test]
fn no_folder_and_no_link_is_clean_and_so_is_a_dangling_link() {
    let (_root, layout) = rig();
    write(&layout.code.join("a/1.0.0/src/app.py"), "x");
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "a"),
        Ok(Removed::Done)
    );
    gone(&layout, "a");
    // The data folder is made when the app first starts: a link whose folder
    // is not there (or is gone) is cleaned, not an error.
    write(&layout.code.join("b/1.0.0/src/app.py"), "x");
    fs::create_dir_all(&layout.data).unwrap();
    symlink(layout.private.join("b"), layout.data.join("b")).unwrap();
    assert_eq!(
        remove(&layout, &mut fx, &mut skills, "b"),
        Ok(Removed::Done)
    );
    gone(&layout, "b");
}

#[test]
fn an_app_that_is_not_there_is_a_clean_no_with_no_effect() {
    let (_root, layout) = rig();
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
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
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
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
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
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
            ..Default::default()
        };
        let mut skills = FakeCompanions::default();
        let first = remove(&layout, &mut fx, &mut skills, "x");
        if first.is_ok() {
            gone(&layout, "x");
            continue;
        }
        assert!(has(&layout.state, "x", Mark::Removing), "call {at}");
        let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
        assert_eq!(
            remove(&layout, &mut fx, &mut skills, "x"),
            Ok(Removed::Done),
            "call {at}"
        );
        gone(&layout, "x");
    }
    let (_root, layout) = rig();
    installed(&layout, "x");
    let mut fx = FakeEffects::default();
    let mut skills = FakeCompanions {
        fail: true,
        ..Default::default()
    };
    assert!(remove(&layout, &mut fx, &mut skills, "x").is_err());
    assert!(has(&layout.state, "x", Mark::Removing));
    let mut skills = FakeCompanions::default();
    assert_eq!(
        remove(&layout, &mut FakeEffects::default(), &mut skills, "x"),
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
    let mut fx = FakeEffects::default();
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
    let mut cut = FakeEffects::default();
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
    let mut fx = FakeEffects::default();
    let report = sweep(&layout, &mut fx, &mut skills);
    assert_eq!(report.resumed_removes, ["x"]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    gone(&layout, "x");
}
