use lamp_apps::install::install;
use lamp_apps::layout::Layout;
use lamp_apps::manage::{BUSY_APP, list, remove_with_yes, start, start_all, stop};
use lamp_apps::package::{AppPackage, AppSpec, sha256_hex};
use lamp_apps::remove::{NOT_INSTALLED, Removed};
use lamp_apps::state::{Mark, has, set};
use lamp_apps::testing::{FakeCompanions, FakeEffects, FakeYes, StubReader};
use lamp_apps::yes::{Op, YesKey};
use std::collections::BTreeMap;

fn package(name: &str, net: bool) -> AppPackage {
    AppPackage {
        app: AppSpec {
            name: name.into(),
            title: "T".into(),
            version: "1.0.0".into(),
            min_build: 1,
            net,
            logs: false,
            grants: vec!["hear".into(), "speak".into()],
            entry: "app.main".into(),
        },
        files: BTreeMap::from([("src/app/main.py".to_owned(), b"x".to_vec())]),
        skills: vec![],
    }
}

fn key(op: Op, name: &str, bytes: &[u8], grants: &[&str], net: bool) -> YesKey {
    YesKey {
        op,
        name: name.into(),
        version: "1.0.0".into(),
        sha256: sha256_hex(bytes),
        grants: grants.iter().map(|g| (*g).to_owned()).collect(),
        net,
        logs: false,
    }
}

struct Rig {
    _root: tempfile::TempDir,
    layout: Layout,
    fx: FakeEffects,
    skills: FakeCompanions,
}

/// A lamp with the named apps installed (net or not), and no calls yet.
fn rig(apps: &[(&str, bool)]) -> Rig {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    let mut fx = FakeEffects::default();
    let mut skills = FakeCompanions::default();
    for (name, net) in apps {
        let bytes = format!("bytes {name}").into_bytes();
        let mut yes = FakeYes::granting(vec![key(
            Op::Install,
            name,
            &bytes,
            &["hear", "speak"],
            *net,
        )]);
        install(
            &layout,
            &mut fx,
            &mut skills,
            &mut yes,
            &StubReader::new(package(name, *net)),
            &bytes,
            Some(200),
        )
        .unwrap();
    }
    fx.calls.clear();
    Rig {
        _root: root,
        layout,
        fx,
        skills,
    }
}

const A: &str = "lamp-app@alpha.service";
const A_NET: &str = "lamp-app-net@alpha.service";

fn line(verb: &str, unit: &str) -> String {
    format!("/usr/bin/systemctl {verb} -- {unit}")
}

#[test]
fn stop_marks_first_and_stops_both_variants() {
    let mut r = rig(&[("alpha", false)]);
    stop(&r.layout, &mut r.fx, "alpha").unwrap();
    assert!(has(&r.layout.state, "alpha", Mark::Stopped));
    assert_eq!(r.fx.calls, [line("stop", A), line("stop", A_NET)]);
}

#[test]
fn a_cut_stop_is_still_a_stop() {
    let mut r = rig(&[("alpha", false)]);
    r.fx.fail_at = Some(0);
    assert!(stop(&r.layout, &mut r.fx, "alpha").is_err());
    assert!(has(&r.layout.state, "alpha", Mark::Stopped));
}

#[test]
fn start_stops_the_other_variant_then_starts_and_forgets_the_stop() {
    let mut r = rig(&[("alpha", true)]);
    set(&r.layout.state, "alpha", Mark::Stopped).unwrap();
    start(&r.layout, &mut r.fx, "alpha").unwrap();
    // The shared cap is asked again before anything is stopped or started.
    assert_eq!(
        r.fx.calls,
        [
            "systemctl set-property --runtime lampos-apps.slice CPUWeight=50 IOWeight=50 MemoryHigh=192M MemoryMax=256M TasksMax=128".to_owned(),
            line("stop", A),
            line("reset-failed", A_NET),
            line("start", A_NET)
        ]
    );
    assert!(!has(&r.layout.state, "alpha", Mark::Stopped));
}

/// After 5 crashes in 600 s the unit is `start-limit-hit` and systemd refuses a start until the window passes:
/// the owner's Start clears the failed state first, the unit it is about to start, before the start. A unit that
/// is not loaded (exit 1, "not loaded") is nothing to clear, and the start goes on.
#[test]
fn start_clears_the_failed_state_of_the_unit_just_before_starting_it() {
    let mut r = rig(&[("alpha", false)]);
    start(&r.layout, &mut r.fx, "alpha").unwrap();
    let calls = &r.fx.calls;
    let reset = calls
        .iter()
        .position(|c| *c == line("reset-failed", A))
        .unwrap();
    let start_at = calls.iter().position(|c| *c == line("start", A)).unwrap();
    assert_eq!(start_at, reset + 1, "{calls:?}");
    let mut r = rig(&[("alpha", false)]);
    let reset = line("reset-failed", A);
    r.fx.exits.push((reset.clone(), 1));
    r.fx.says.push((
        reset,
        "Failed to reset failed state of unit lamp-app@alpha.service: Unit lamp-app@alpha.service not loaded.".into(),
    ));
    start(&r.layout, &mut r.fx, "alpha").unwrap();
    assert_eq!(r.fx.calls.last(), Some(&line("start", A)));
}

#[test]
fn a_failed_start_stays_stopped() {
    let mut r = rig(&[("alpha", false)]);
    set(&r.layout.state, "alpha", Mark::Stopped).unwrap();
    // call 0 is the shared cap, 1 the stop of the other variant, 2 the reset of the failed state, 3 the start
    r.fx.fail_at = Some(3);
    assert!(start(&r.layout, &mut r.fx, "alpha").is_err());
    assert!(has(&r.layout.state, "alpha", Mark::Stopped));
}

/// An owner start or an install's start with the shared cap not set (it lives in /run: a failed set, or a lost
/// one) must not run the app uncapped: refused, nothing stopped or started, the stop mark kept.
#[test]
fn start_refuses_an_app_when_the_shared_cap_cannot_be_set() {
    let mut r = rig(&[("alpha", false)]);
    set(&r.layout.state, "alpha", Mark::Stopped).unwrap();
    r.fx.fail_at = Some(0);
    let error = start(&r.layout, &mut r.fx, "alpha").unwrap_err();
    assert!(error.contains("shared limit"), "{error}");
    assert_eq!(r.fx.calls.len(), 1, "{:?}", r.fx.calls);
    assert!(has(&r.layout.state, "alpha", Mark::Stopped));
}

#[test]
fn start_and_stop_refuse_an_app_being_changed_or_not_there() {
    let mut r = rig(&[("alpha", false)]);
    assert_eq!(
        start(&r.layout, &mut r.fx, "ghost"),
        Err(NOT_INSTALLED.into())
    );
    assert!(start(&r.layout, &mut r.fx, "../x").is_err());
    for mark in [Mark::Removing, Mark::Installing] {
        set(&r.layout.state, "alpha", mark).unwrap();
        assert_eq!(start(&r.layout, &mut r.fx, "alpha"), Err(BUSY_APP.into()));
        assert_eq!(stop(&r.layout, &mut r.fx, "alpha"), Err(BUSY_APP.into()));
        lamp_apps::state::clear(&r.layout.state, "alpha", mark).unwrap();
    }
    assert!(r.fx.calls.is_empty());
}

#[test]
fn start_all_starts_the_running_ones_only() {
    let mut r = rig(&[
        ("alpha", false),
        ("beta", true),
        ("gamma", false),
        ("delta", false),
    ]);
    set(&r.layout.state, "gamma", Mark::Stopped).unwrap();
    set(&r.layout.state, "delta", Mark::Removing).unwrap();
    let (started, errors) = start_all(&r.layout, &mut r.fx);
    assert_eq!(started, ["alpha", "beta"]);
    assert!(errors.is_empty());
    // The shared cap is set before the first app starts.
    assert_eq!(
        r.fx.calls,
        [
            "systemctl set-property --runtime lampos-apps.slice CPUWeight=50 IOWeight=50 MemoryHigh=192M MemoryMax=256M TasksMax=128"
                .to_owned(),
            line("start", A),
            line("start", "lamp-app-net@beta.service")
        ]
    );
}

/// The slice has no unit file, so its cap is set at runtime; if that fails no app starts (fails closed).
#[test]
fn start_all_starts_no_app_when_the_shared_cap_cannot_be_set() {
    let mut r = rig(&[("alpha", false), ("beta", false)]);
    r.fx.fail_at = Some(0);
    let (started, errors) = start_all(&r.layout, &mut r.fx);
    assert!(started.is_empty());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("no app starts"), "{errors:?}");
    assert_eq!(
        r.fx.calls.len(),
        1,
        "nothing else was asked: {:?}",
        r.fx.calls
    );
}

#[test]
fn start_all_reports_a_failure_and_goes_on() {
    let mut r = rig(&[("alpha", false), ("beta", false)]);
    // call 0 is the shared cap; call 1 is alpha's start
    r.fx.fail_at = Some(1);
    let (started, errors) = start_all(&r.layout, &mut r.fx);
    assert_eq!(started, ["beta"]);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("alpha: "));
}

/// 0c89, 20 Nov 16:04: an installed app whose unit is not loaded (its template not placed yet by the old updater
/// that ran the round, #8115) makes `systemctl start` exit 5. `start_all` reports it and goes on to the next app
/// (the OS's own units are the round's to judge), and the boot start's exit never fails because of it
/// (`main.rs`, checked below).
#[test]
fn start_all_goes_on_past_an_app_whose_unit_is_not_loaded() {
    let mut r = rig(&[("alpha", false), ("beta", false)]);
    r.fx.exits.push((line("start", A), 5));
    let (started, errors) = start_all(&r.layout, &mut r.fx);
    assert_eq!(started, ["beta"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("alpha: "), "{errors:?}");
    assert!(errors[0].contains("exit 5"), "{errors:?}");
}

/// `start-all` never exits non-zero for an app that will not start: the unit is restarted by an update's round, and
/// a failed unit puts the whole round back (0c89, 20 Nov 16:04). The arm of `main` that runs it returns success
/// after logging each error, and nowhere in that arm after `start_all(` is a failure returned.
#[test]
fn the_boot_start_never_fails_its_unit_for_an_app_that_will_not_start() {
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs")).unwrap();
    let arm = source
        .split("Some(\"start-all\") =>")
        .nth(1)
        .expect("the start-all arm");
    let after = arm
        .split("start_all(&layout, &mut fx)")
        .nth(1)
        .expect("the call")
        .split("_ => {")
        .next()
        .unwrap();
    assert!(after.contains("ExitCode::SUCCESS"), "{after}");
    assert!(!after.contains("FAILURE"), "{after}");
}

#[test]
fn start_all_with_nothing_installed_does_nothing() {
    let mut r = rig(&[]);
    assert_eq!(start_all(&r.layout, &mut r.fx), (vec![], vec![]));
}

#[test]
fn list_says_version_grants_net_and_stopped() {
    let r = rig(&[("alpha", false), ("beta", true)]);
    set(&r.layout.state, "beta", Mark::Stopped).unwrap();
    let all = list(&r.layout);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].name, "alpha");
    assert_eq!(all[0].version, "1.0.0");
    assert_eq!(all[0].grants, ["hear", "speak"]);
    assert!(!all[0].net && !all[0].stopped);
    assert!(all[1].net && all[1].stopped);
    set(&r.layout.state, "alpha", Mark::Installing).unwrap();
    assert_eq!(list(&r.layout).len(), 1);
    assert!(r.fx.calls.is_empty());
}

fn remove_yes(name: &str) -> FakeYes {
    FakeYes::granting(vec![key(
        Op::Remove,
        name,
        format!("bytes {name}").as_bytes(),
        &[],
        false,
    )])
}

#[test]
fn a_remove_uses_the_yes_for_the_installed_version_then_removes() {
    let mut r = rig(&[("alpha", false)]);
    let mut yes = remove_yes("alpha");
    let done = remove_with_yes(&r.layout, &mut r.fx, &mut r.skills, &mut yes, "alpha");
    assert_eq!(done, Ok(Removed::Done));
    assert_eq!(yes.asked, 1);
    assert!(!r.layout.code.join("alpha").exists());
}

#[test]
fn a_remove_with_no_yes_touches_nothing() {
    let mut r = rig(&[("alpha", false)]);
    let mut yes = FakeYes::granting(vec![]);
    assert!(remove_with_yes(&r.layout, &mut r.fx, &mut r.skills, &mut yes, "alpha").is_err());
    assert!(r.layout.code.join("alpha").exists());
    assert!(!has(&r.layout.state, "alpha", Mark::Removing));
    assert!(r.fx.calls.is_empty());
}

#[test]
fn a_remove_of_another_apps_yes_is_refused() {
    let mut r = rig(&[("alpha", false), ("beta", false)]);
    let mut yes = remove_yes("beta");
    assert!(remove_with_yes(&r.layout, &mut r.fx, &mut r.skills, &mut yes, "alpha").is_err());
    assert!(r.layout.code.join("alpha").exists());
}

#[test]
fn a_remove_of_nothing_asks_no_yes() {
    let mut r = rig(&[]);
    let mut yes = FakeYes::granting(vec![]);
    let done = remove_with_yes(&r.layout, &mut r.fx, &mut r.skills, &mut yes, "ghost");
    assert_eq!(done, Err(NOT_INSTALLED.into()));
    assert_eq!(yes.asked, 0);
}

#[test]
fn a_cut_remove_is_resumed_without_a_second_yes() {
    let mut r = rig(&[("alpha", false)]);
    let mut yes = remove_yes("alpha");
    r.fx.fail_at = Some(0);
    assert!(remove_with_yes(&r.layout, &mut r.fx, &mut r.skills, &mut yes, "alpha").is_err());
    assert!(has(&r.layout.state, "alpha", Mark::Removing));
    r.fx.fail_at = None;
    let mut none = FakeYes::granting(vec![]);
    let done = remove_with_yes(&r.layout, &mut r.fx, &mut r.skills, &mut none, "alpha");
    assert_eq!(done, Ok(Removed::Done));
    assert_eq!(none.asked, 0);
}

#[test]
fn a_remove_during_an_install_is_refused_before_the_yes() {
    let mut r = rig(&[("alpha", false)]);
    set(&r.layout.state, "alpha", Mark::Installing).unwrap();
    let mut yes = remove_yes("alpha");
    assert_eq!(
        remove_with_yes(&r.layout, &mut r.fx, &mut r.skills, &mut yes, "alpha"),
        Err(BUSY_APP.into())
    );
    assert_eq!(yes.asked, 0);
}
