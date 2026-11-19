use lamp_apps::layout::Layout;
use lamp_apps::state::{Mark, has, set};
use lamp_apps::sweep::{sweep, sweep_staging};
use lamp_apps::testing::{FakeCompanions, FakeEffects};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::time::{Duration, SystemTime};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn rig() -> (tempfile::TempDir, Layout) {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    (root, layout)
}

#[test]
fn an_interrupted_remove_is_finished() {
    let (_root, layout) = rig();
    write(&layout.code.join("x/1.0.0/src/app.py"), "x");
    write(&layout.data_real("x/f"), "f");
    set(&layout.state, "x", Mark::Removing).unwrap();
    let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
    let report = sweep(&layout, &mut fx, &mut skills);
    assert_eq!(report.resumed_removes, ["x"]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!layout.code.join("x").exists());
    assert!(!layout.data_real("x").exists());
    assert!(!has(&layout.state, "x", Mark::Removing));
}

#[test]
fn an_install_cut_before_the_link_leaves_nothing() {
    let (_root, layout) = rig();
    write(&layout.code.join("x/.tmp-1/src/app.py"), "x");
    write(&layout.code.join("x/1.0.0/src/app.py"), "x");
    set(&layout.state, "x", Mark::Installing).unwrap();
    let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
    let report = sweep(&layout, &mut fx, &mut skills);
    assert_eq!(report.cleaned_installs, ["x"]);
    assert!(!layout.code.join("x").exists());
    assert_eq!(skills.removed, ["x"], "the companions placed first go too");
    assert!(!has(&layout.state, "x", Mark::Installing));
}

#[test]
fn an_update_cut_after_the_link_keeps_the_current_version_only() {
    let (_root, layout) = rig();
    write(&layout.code.join("x/.tmp-2/f"), "x");
    write(&layout.code.join("x/1.0.0/src/app.py"), "old");
    write(&layout.code.join("x/1.1.0/src/app.py"), "new");
    symlink("1.1.0", layout.code.join("x/current")).unwrap();
    set(&layout.state, "x", Mark::Installing).unwrap();
    let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
    sweep(&layout, &mut fx, &mut skills);
    assert!(layout.code.join("x/1.1.0/src/app.py").exists());
    assert!(!layout.code.join("x/1.0.0").exists());
    assert!(!layout.code.join("x/.tmp-2").exists());
    assert!(
        skills.removed.is_empty(),
        "an installed app keeps its skills"
    );
    assert!(!has(&layout.state, "x", Mark::Installing));
}

#[test]
fn a_stale_stopped_mark_goes_and_a_live_one_stays() {
    let (_root, layout) = rig();
    set(&layout.state, "gone", Mark::Stopped).unwrap();
    write(&layout.code.join("here/1.0.0/f"), "x");
    set(&layout.state, "here", Mark::Stopped).unwrap();
    let (mut fx, mut skills) = (FakeEffects::cleaning(&layout), FakeCompanions::default());
    sweep(&layout, &mut fx, &mut skills);
    assert!(!has(&layout.state, "gone", Mark::Stopped));
    assert!(has(&layout.state, "here", Mark::Stopped));
}

#[test]
fn one_failed_resume_does_not_stop_the_rest() {
    let (_root, layout) = rig();
    for name in ["a", "b"] {
        write(&layout.code.join(name).join("1.0.0/f"), "x");
        set(&layout.state, name, Mark::Removing).unwrap();
    }
    let mut fx = FakeEffects {
        fail_at: Some(0),
        ..FakeEffects::cleaning(&layout)
    };
    let mut skills = FakeCompanions::default();
    let report = sweep(&layout, &mut fx, &mut skills);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert_eq!(report.resumed_removes.len(), 1);
}

#[test]
fn stale_staged_files_go_by_age_then_by_count_and_nothing_is_followed() {
    let (root, layout) = rig();
    let outside = root.path().join("outside");
    write(&outside, "keep");
    fs::create_dir_all(&layout.staging).unwrap();
    let now = SystemTime::now();
    let make = |name: &str, age_s: u64| {
        let path = layout.staging.join(name);
        fs::write(&path, name).unwrap();
        let file = fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(now - Duration::from_secs(age_s)).unwrap();
    };
    make("old.tar", 7200);
    make("c.tar", 30);
    make("b.tar", 20);
    make("a.tar", 10);
    symlink(&outside, layout.staging.join("link")).unwrap();
    fs::create_dir(layout.staging.join("sub")).unwrap();
    let removed = sweep_staging(&layout.staging, now, Duration::from_secs(3600), 2);
    assert_eq!(removed, 3, "old, the link, and the oldest over the cap (c)");
    let left: Vec<_> = {
        let mut names: Vec<_> = fs::read_dir(&layout.staging)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };
    assert_eq!(left, ["a.tar", "b.tar", "sub"]);
    assert_eq!(fs::read_to_string(&outside).unwrap(), "keep");
}
