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

const A: &str = "lampapp@alpha.service";
const A_NET: &str = "lampapp-net@alpha.service";

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
    assert_eq!(r.fx.calls, [line("stop", A), line("start", A_NET)]);
    assert!(!has(&r.layout.state, "alpha", Mark::Stopped));
}

#[test]
fn a_failed_start_stays_stopped() {
    let mut r = rig(&[("alpha", false)]);
    set(&r.layout.state, "alpha", Mark::Stopped).unwrap();
    r.fx.fail_at = Some(1);
    assert!(start(&r.layout, &mut r.fx, "alpha").is_err());
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
    assert_eq!(
        r.fx.calls,
        [line("start", A), line("start", "lampapp-net@beta.service")]
    );
}

#[test]
fn start_all_reports_a_failure_and_goes_on() {
    let mut r = rig(&[("alpha", false), ("beta", false)]);
    r.fx.fail_at = Some(0);
    let (started, errors) = start_all(&r.layout, &mut r.fx);
    assert_eq!(started, ["beta"]);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("alpha: "));
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
