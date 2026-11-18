use lamp_apps::install::{ALREADY, install, installed_record};
use lamp_apps::layout::Layout;
use lamp_apps::lock::{AppsLock, BUSY};
use lamp_apps::package::{AppPackage, AppSpec, SkillFolder, sha256_hex};
use lamp_apps::remove::{Removed, remove};
use lamp_apps::state::{Mark, has, set};
use lamp_apps::sweep::sweep;
use lamp_apps::testing::{FakeCompanions, FakeEffects, FakeYes, StubReader};
use lamp_apps::yes::{Op, YesKey};
use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

fn package(name: &str, version: &str, grants: &[&str], net: bool) -> AppPackage {
    let mut files = BTreeMap::new();
    files.insert("src/app/main.py".to_owned(), b"print('hi')\n".to_vec());
    files.insert("assets/ok.wav".to_owned(), b"RIFF".to_vec());
    files.insert("lamp-skill.json".to_owned(), b"{}".to_vec());
    AppPackage {
        app: AppSpec {
            name: name.into(),
            title: "English lessons".into(),
            version: version.into(),
            min_build: 100,
            net,
            grants: grants.iter().map(|g| (*g).to_owned()).collect(),
            entry: "app.main".into(),
        },
        files,
        skills: vec![SkillFolder {
            name: "english-tutor".into(),
            files: BTreeMap::from([("SKILL.toml".to_owned(), b"x".to_vec())]),
        }],
    }
}

/// The bytes the staged file holds (any bytes: the stub reader does not
/// parse them) and the yes key the owner's tap would have made for them.
fn bytes_of(tag: &str) -> Vec<u8> {
    format!("package bytes {tag}").into_bytes()
}

fn key(op: Op, package: &AppPackage, bytes: &[u8], new_grants: &[&str]) -> YesKey {
    YesKey {
        op,
        name: package.app.name.clone(),
        version: package.app.version.clone(),
        sha256: sha256_hex(bytes),
        grants: new_grants.iter().map(|g| (*g).to_owned()).collect(),
    }
}

struct Rig {
    _root: tempfile::TempDir,
    layout: Layout,
    fx: FakeEffects,
    skills: FakeCompanions,
}

fn rig() -> Rig {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    let skills = FakeCompanions {
        lock_probe: Some(layout.state.clone()),
        ..Default::default()
    };
    Rig {
        _root: root,
        layout,
        fx: FakeEffects::default(),
        skills,
    }
}

fn put(
    rig: &mut Rig,
    package: &AppPackage,
    tag: &str,
    op: Op,
    new_grants: &[&str],
) -> Result<lamp_apps::install::Installed, String> {
    let bytes = bytes_of(tag);
    let mut yes = FakeYes::granting(vec![key(op, package, &bytes, new_grants)]);
    install(
        &rig.layout,
        &mut rig.fx,
        &mut rig.skills,
        &mut yes,
        &StubReader::new(package.clone()),
        &bytes,
        Some(200),
    )
}

fn nothing_written(rig: &Rig, name: &str) {
    assert!(!rig.layout.code.join(name).exists(), "code");
    assert!(
        rig.layout
            .state
            .read_dir()
            .map(|mut d| d.all(|e| e.unwrap().file_name() == ".lock"))
            .unwrap_or(true),
        "marks"
    );
    assert!(rig.skills.placed.is_empty(), "skills");
    assert!(rig.fx.calls.is_empty(), "effects");
}

#[test]
fn a_fresh_install_lays_everything_and_starts_the_app() {
    let mut rig = rig();
    let pkg = package("english-lessons", "1.0.0", &["speak", "hear"], false);
    let done = put(&mut rig, &pkg, "a", Op::Install, &["hear", "speak"]).unwrap();
    assert_eq!(done.unit, "lampapp@english-lessons.service");
    assert!(done.started);
    let app = rig.layout.code.join("english-lessons");
    assert_eq!(
        fs::read_to_string(app.join("1.0.0/src/app/main.py")).unwrap(),
        "print('hi')\n"
    );
    assert_eq!(
        fs::read_to_string(app.join("1.0.0/entry.env")).unwrap(),
        "ENTRY=app.main\n"
    );
    assert_eq!(
        fs::read_link(app.join("current")).unwrap().to_str(),
        Some("1.0.0")
    );
    assert_eq!(
        fs::read_to_string(app.join("current/assets/ok.wav")).unwrap(),
        "RIFF"
    );
    let record = installed_record(&rig.layout, "english-lessons").unwrap();
    assert_eq!((record.version.as_str(), record.net), ("1.0.0", false));
    assert_eq!(
        rig.fx.calls,
        ["/usr/bin/systemctl restart -- lampapp@english-lessons.service"]
    );
    assert_eq!(
        rig.skills.placed,
        [(
            "english-lessons".to_owned(),
            vec!["english-tutor".to_owned()]
        )]
    );
    for mark in [Mark::Installing, Mark::Removing, Mark::Stopped] {
        assert!(!has(&rig.layout.state, "english-lessons", mark));
    }
    assert!(
        !rig.skills.violation,
        "the skills were placed while the apps lock was held"
    );
}

#[test]
fn net_in_the_manifest_picks_the_net_unit() {
    let mut rig = rig();
    let pkg = package("x", "1.0.0", &["speak"], true);
    put(&mut rig, &pkg, "a", Op::Install, &["speak"]).unwrap();
    assert_eq!(
        rig.fx.calls,
        ["/usr/bin/systemctl restart -- lampapp-net@x.service"]
    );
}

#[test]
fn nothing_is_written_without_the_yes_and_a_changed_file_after_it_installs_nothing() {
    let mut rig = rig();
    let pkg = package("x", "1.0.0", &["speak"], false);
    // No yes at all.
    let bytes = bytes_of("a");
    let mut none = FakeYes::granting(vec![]);
    let error = install(
        &rig.layout,
        &mut rig.fx,
        &mut rig.skills,
        &mut none,
        &StubReader::new(pkg.clone()),
        &bytes,
        Some(200),
    )
    .unwrap_err();
    assert!(error.contains("yes"), "{error}");
    nothing_written(&rig, "x");
    // The owner said yes to the bytes of "a"; the file at the url is now "b".
    let mut yes = FakeYes::granting(vec![key(Op::Install, &pkg, &bytes_of("a"), &["speak"])]);
    let changed = bytes_of("b");
    assert!(
        install(
            &rig.layout,
            &mut rig.fx,
            &mut rig.skills,
            &mut yes,
            &StubReader::new(pkg.clone()),
            &changed,
            Some(200)
        )
        .is_err()
    );
    nothing_written(&rig, "x");
    // The yes is still there for the real bytes, and is used up by them.
    assert!(
        install(
            &rig.layout,
            &mut rig.fx,
            &mut rig.skills,
            &mut yes,
            &StubReader::new(pkg.clone()),
            &bytes,
            Some(200)
        )
        .is_ok()
    );
    let second = package("x", "1.0.1", &["speak"], false);
    let error = install(
        &rig.layout,
        &mut rig.fx,
        &mut rig.skills,
        &mut yes,
        &StubReader::new(second),
        &bytes,
        Some(200),
    )
    .unwrap_err();
    assert!(!error.is_empty());
}

#[test]
fn the_yes_is_single_use_and_keyed_to_op_name_version_and_grants() {
    let pkg = package("x", "1.0.0", &["speak"], false);
    let bytes = bytes_of("a");
    for wrong in [
        key(Op::Update, &pkg, &bytes, &["speak"]),
        key(
            Op::Install,
            &package("y", "1.0.0", &["speak"], false),
            &bytes,
            &["speak"],
        ),
        key(
            Op::Install,
            &package("x", "1.0.1", &["speak"], false),
            &bytes,
            &["speak"],
        ),
        key(Op::Install, &pkg, &bytes, &[]),
        key(Op::Install, &pkg, &bytes, &["speak", "hear"]),
    ] {
        let mut rig = rig();
        let mut yes = FakeYes::granting(vec![wrong]);
        assert!(
            install(
                &rig.layout,
                &mut rig.fx,
                &mut rig.skills,
                &mut yes,
                &StubReader::new(pkg.clone()),
                &bytes,
                Some(200)
            )
            .is_err()
        );
        nothing_written(&rig, "x");
    }
}

#[test]
fn a_refusal_before_the_yes_never_spends_it() {
    let mut rig = rig();
    let mut bad = package("x", "1.0.0", &["speak"], false);
    bad.app.grants = vec!["calendar-read".into()];
    let bytes = bytes_of("a");
    let mut yes = FakeYes::granting(vec![key(Op::Install, &bad, &bytes, &["calendar-read"])]);
    assert!(
        install(
            &rig.layout,
            &mut rig.fx,
            &mut rig.skills,
            &mut yes,
            &StubReader::new(bad.clone()),
            &bytes,
            Some(200)
        )
        .is_err()
    );
    assert_eq!(yes.asked, 0, "the yes was not asked for");
    // The build is unknown: refused, the yes is kept.
    let ok = package("x", "1.0.0", &["speak"], false);
    let mut yes = FakeYes::granting(vec![key(Op::Install, &ok, &bytes, &["speak"])]);
    assert!(
        install(
            &rig.layout,
            &mut rig.fx,
            &mut rig.skills,
            &mut yes,
            &StubReader::new(ok.clone()),
            &bytes,
            None
        )
        .is_err()
    );
    assert_eq!(yes.asked, 0);
    nothing_written(&rig, "x");
}

#[test]
fn a_package_file_outside_src_and_assets_or_with_a_bad_path_is_refused_before_the_yes() {
    for bad in [
        "../evil",
        "/etc/passwd",
        "src/../../evil",
        "src//x.py",
        "src/./x.py",
        "other/x.py",
        "entry.env",
        "lamp-apps.json",
        "src/a b.py",
        "src/.hidden",
        "src/x\n.py",
        "",
        "src/",
        "src/a/b/c/d/e/f/g/h/i/j.py",
    ] {
        let mut rig = rig();
        let mut pkg = package("x", "1.0.0", &["speak"], false);
        pkg.files.insert(bad.to_owned(), b"x".to_vec());
        let bytes = bytes_of("a");
        let mut yes = FakeYes::granting(vec![key(Op::Install, &pkg, &bytes, &["speak"])]);
        assert!(
            install(
                &rig.layout,
                &mut rig.fx,
                &mut rig.skills,
                &mut yes,
                &StubReader::new(pkg),
                &bytes,
                Some(200)
            )
            .is_err(),
            "{bad:?}"
        );
        assert_eq!(yes.asked, 0, "{bad:?}");
        nothing_written(&rig, "x");
    }
    let mut rig = rig();
    let mut pkg = package("x", "1.0.0", &["speak"], false);
    pkg.skills[0].files.insert("../x".into(), b"x".to_vec());
    let bytes = bytes_of("a");
    let mut yes = FakeYes::granting(vec![key(Op::Install, &pkg, &bytes, &["speak"])]);
    assert!(
        install(
            &rig.layout,
            &mut rig.fx,
            &mut rig.skills,
            &mut yes,
            &StubReader::new(pkg),
            &bytes,
            Some(200)
        )
        .is_err()
    );
    assert_eq!(yes.asked, 0);
}

#[test]
fn a_name_a_core_skill_has_is_refused() {
    let mut rig = rig();
    rig.skills.taken = vec!["x".into()];
    let pkg = package("x", "1.0.0", &["speak"], false);
    let bytes = bytes_of("a");
    let mut yes = FakeYes::granting(vec![key(Op::Install, &pkg, &bytes, &["speak"])]);
    assert!(
        install(
            &rig.layout,
            &mut rig.fx,
            &mut rig.skills,
            &mut yes,
            &StubReader::new(pkg),
            &bytes,
            Some(200)
        )
        .is_err()
    );
    assert_eq!(yes.asked, 0);
}

#[test]
fn an_update_switches_the_link_and_keeps_only_the_new_version_after_a_sweep() {
    let mut rig = rig();
    put(
        &mut rig,
        &package("x", "1.0.0", &["speak"], false),
        "a",
        Op::Install,
        &["speak"],
    )
    .unwrap();
    let new = package("x", "1.1.0", &["speak"], false);
    let done = put(&mut rig, &new, "b", Op::Update, &[]).unwrap();
    assert!(done.started);
    let app = rig.layout.code.join("x");
    assert_eq!(
        fs::read_link(app.join("current")).unwrap().to_str(),
        Some("1.1.0")
    );
    assert!(
        app.join("1.0.0").exists(),
        "the old one beside until the sweep"
    );
    assert_eq!(installed_record(&rig.layout, "x").unwrap().version, "1.1.0");
}

#[test]
fn an_update_that_asks_more_grants_needs_the_yes_for_exactly_the_new_ones() {
    let mut rig = rig();
    put(
        &mut rig,
        &package("x", "1.0.0", &["speak"], false),
        "a",
        Op::Install,
        &["speak"],
    )
    .unwrap();
    let calls = rig.fx.calls.len();
    let more = package("x", "1.1.0", &["speak", "hear", "light"], false);
    let bytes = bytes_of("b");
    // The yes covers `hear` only; the update also asks `light`.
    let mut yes = FakeYes::granting(vec![key(Op::Update, &more, &bytes, &["hear"])]);
    assert!(
        install(
            &rig.layout,
            &mut rig.fx,
            &mut rig.skills,
            &mut yes,
            &StubReader::new(more.clone()),
            &bytes,
            Some(200)
        )
        .is_err()
    );
    let app = rig.layout.code.join("x");
    assert_eq!(
        fs::read_link(app.join("current")).unwrap().to_str(),
        Some("1.0.0"),
        "the old version keeps running"
    );
    assert!(!app.join("1.1.0").exists());
    assert_eq!(rig.fx.calls.len(), calls, "nothing restarted");
    let mut yes = FakeYes::granting(vec![key(Op::Update, &more, &bytes, &["hear", "light"])]);
    assert!(
        install(
            &rig.layout,
            &mut rig.fx,
            &mut rig.skills,
            &mut yes,
            &StubReader::new(more),
            &bytes,
            Some(200)
        )
        .is_ok()
    );
}

#[test]
fn an_older_or_the_same_version_is_refused() {
    let mut rig = rig();
    put(
        &mut rig,
        &package("x", "1.2.0", &["speak"], false),
        "a",
        Op::Install,
        &["speak"],
    )
    .unwrap();
    let older = package("x", "1.1.9", &["speak"], false);
    assert!(put(&mut rig, &older, "b", Op::Update, &[]).is_err());
    let same = package("x", "1.2.0", &["speak"], false);
    assert_eq!(
        put(&mut rig, &same, "c", Op::Update, &[]).unwrap_err(),
        ALREADY
    );
}

#[test]
fn an_update_never_starts_an_app_the_owner_stopped() {
    let mut rig = rig();
    put(
        &mut rig,
        &package("x", "1.0.0", &["speak"], false),
        "a",
        Op::Install,
        &["speak"],
    )
    .unwrap();
    set(&rig.layout.state, "x", Mark::Stopped).unwrap();
    rig.fx.calls.clear();
    let done = put(
        &mut rig,
        &package("x", "1.1.0", &["speak"], false),
        "b",
        Op::Update,
        &[],
    )
    .unwrap();
    assert!(!done.started);
    assert!(rig.fx.calls.is_empty(), "{:?}", rig.fx.calls);
    assert!(has(&rig.layout.state, "x", Mark::Stopped));
}

#[test]
fn a_fresh_install_clears_a_stale_stopped_mark_and_starts() {
    let mut rig = rig();
    set(&rig.layout.state, "x", Mark::Stopped).unwrap();
    let done = put(
        &mut rig,
        &package("x", "1.0.0", &["speak"], false),
        "a",
        Op::Install,
        &["speak"],
    )
    .unwrap();
    assert!(done.started);
    assert!(!has(&rig.layout.state, "x", Mark::Stopped));
}

#[test]
fn flipping_net_stops_the_other_variant_before_the_new_one_starts() {
    let mut rig = rig();
    put(
        &mut rig,
        &package("x", "1.0.0", &["speak"], false),
        "a",
        Op::Install,
        &["speak"],
    )
    .unwrap();
    rig.fx.calls.clear();
    let done = put(
        &mut rig,
        &package("x", "1.1.0", &["speak"], true),
        "b",
        Op::Update,
        &[],
    )
    .unwrap();
    assert_eq!(done.unit, "lampapp-net@x.service");
    assert_eq!(
        rig.fx.calls,
        [
            "/usr/bin/systemctl stop -- lampapp@x.service",
            "/usr/bin/systemctl restart -- lampapp-net@x.service"
        ]
    );
    // Back again.
    rig.fx.calls.clear();
    put(
        &mut rig,
        &package("x", "1.2.0", &["speak"], false),
        "c",
        Op::Update,
        &[],
    )
    .unwrap();
    assert_eq!(
        rig.fx.calls,
        [
            "/usr/bin/systemctl stop -- lampapp-net@x.service",
            "/usr/bin/systemctl restart -- lampapp@x.service"
        ]
    );
}

#[test]
fn a_failure_at_any_step_then_a_sweep_and_a_retry_ends_installed_once() {
    // The yes is the first thing that can fail; then the companions; then
    // the start; then the folder itself.
    for case in ["start", "companions", "folder"] {
        let mut rig = rig();
        let pkg = package("x", "1.0.0", &["speak"], false);
        match case {
            "start" => rig.fx.fail_at = Some(0),
            "companions" => rig.skills.fail_place = true,
            _ => {
                fs::create_dir_all(&rig.layout.code).unwrap();
                fs::write(rig.layout.code.join("x"), "a file where the folder goes").unwrap();
            }
        }
        assert!(
            put(&mut rig, &pkg, "a", Op::Install, &["speak"]).is_err(),
            "{case}"
        );
        // What the helper's start-up does next.
        let _ = fs::remove_file(rig.layout.code.join("x"))
            .ok()
            .filter(|_| case == "folder");
        let mut fx = FakeEffects::default();
        let mut skills = FakeCompanions::default();
        sweep(&rig.layout, &mut fx, &mut skills);
        assert!(!has(&rig.layout.state, "x", Mark::Installing), "{case}");
        rig.fx = FakeEffects::default();
        rig.skills = FakeCompanions::default();
        assert!(
            put(&mut rig, &pkg, "a", Op::Install, &["speak"]).is_ok(),
            "{case}"
        );
        assert_eq!(rig.fx.calls.len(), 1, "{case}");
    }
}

#[test]
fn a_failed_fresh_install_leaves_no_code_and_no_companions() {
    let mut rig = rig();
    rig.fx.fail_at = Some(0);
    let pkg = package("x", "1.0.0", &["speak"], false);
    assert!(put(&mut rig, &pkg, "a", Op::Install, &["speak"]).is_err());
    assert!(!rig.layout.code.join("x").exists());
    assert_eq!(rig.skills.removed, ["x"]);
    assert!(!has(&rig.layout.state, "x", Mark::Installing));
}

#[test]
fn a_second_install_while_one_runs_is_told_busy_and_writes_nothing() {
    let mut rig = rig();
    let held = AppsLock::take(&rig.layout.state, Duration::from_secs(2)).unwrap();
    let pkg = package("x", "1.0.0", &["speak"], false);
    assert_eq!(
        put(&mut rig, &pkg, "a", Op::Install, &["speak"]).unwrap_err(),
        BUSY
    );
    assert!(!rig.layout.code.join("x").exists());
    drop(held);
}

#[test]
fn remove_still_works_on_what_install_laid() {
    let mut rig = rig();
    put(
        &mut rig,
        &package("x", "1.0.0", &["speak"], false),
        "a",
        Op::Install,
        &["speak"],
    )
    .unwrap();
    assert_eq!(
        remove(&rig.layout, &mut rig.fx, &mut rig.skills, "x"),
        Ok(Removed::Done)
    );
    assert!(!rig.layout.code.join("x").exists());
    assert!(
        !rig.skills.violation,
        "the companions were removed while the apps lock was held"
    );
}

/// Another caller's whole install of `name`, run on the rig's folders while
/// the first one has let the apps lock go to place its skills.
fn other_install(layout: Layout, pkg: AppPackage, tag: &'static str) -> Box<dyn FnMut()> {
    Box::new(move || {
        let bytes = bytes_of(tag);
        let mut fx = FakeEffects::default();
        let mut skills = FakeCompanions::default();
        let mut yes = FakeYes::granting(vec![key(Op::Install, &pkg, &bytes, &["speak"])]);
        install(
            &layout,
            &mut fx,
            &mut skills,
            &mut yes,
            &StubReader::new(pkg.clone()),
            &bytes,
            Some(200),
        )
        .unwrap();
    })
}

#[test]
fn a_failed_fresh_install_never_removes_another_install_of_the_same_name() {
    let mut rig = rig();
    let pkg = package("x", "1.0.0", &["speak"], false);
    // B starts fresh; A (a repackaged 1.0.0) finishes in the gap; B then
    // fails its re-check.
    let other = package("x", "1.0.0", &["speak"], false);
    rig.skills.on_place = Some(other_install(rig.layout.clone(), other, "a"));
    let error = put(&mut rig, &pkg, "b", Op::Install, &["speak"]).unwrap_err();
    assert!(error.contains(ALREADY), "{error}");
    let app = rig.layout.code.join("x");
    assert_eq!(
        fs::read_link(app.join("current")).unwrap().to_str(),
        Some("1.0.0")
    );
    assert!(app.join("1.0.0/src/app/main.py").is_file());
    assert_eq!(installed_record(&rig.layout, "x").unwrap().version, "1.0.0");
    assert!(
        rig.skills.removed.is_empty(),
        "A's companions were taken: {:?}",
        rig.skills.removed
    );
    assert!(!has(&rig.layout.state, "x", Mark::Installing));
}

#[test]
fn a_remove_in_the_gap_stops_the_install_from_laying_code() {
    let mut rig = rig();
    let pkg = package("x", "1.0.0", &["speak"], false);
    let layout = rig.layout.clone();
    rig.skills.on_place = Some(Box::new(move || {
        set(&layout.state, "x", Mark::Removing).unwrap();
    }));
    let error = put(&mut rig, &pkg, "a", Op::Install, &["speak"]).unwrap_err();
    assert!(error.contains("being changed"), "{error}");
    assert!(!rig.layout.code.join("x/current").exists());
    assert!(rig.fx.calls.is_empty());
}
