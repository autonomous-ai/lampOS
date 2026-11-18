//! With `LAMP_STORE_APPS` not `on`, nothing of the apps acts (the first
//! builder's call on #7813: the socket and the boot start are enabled and
//! their folders made whatever the switch says, so each must do nothing off).
use lamp_apps::ask::{OFF, Parts, answer};
use lamp_apps::layout::Layout;
use lamp_apps::testing::{FakeCompanions, FakeEffects, FakeYes, StubReader};
use lamp_apps::{package::AppPackage, package::AppSpec};
use serde_json::json;
use std::collections::BTreeMap;
use std::process::Command;

/// The real binary's `start-all`, with the switch unset, off, or anything but
/// `on`: it says so, exits success and touches nothing. Were it to run, it
/// would write the launcher under `/opt/lampOS/apps` and fail here, so a
/// regression that ignores the switch cannot pass.
#[test]
fn start_all_with_the_switch_off_starts_nothing() {
    for switch in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-apps-manage"));
        command.arg("start-all").env_clear();
        if let Some(value) = switch {
            command.env("LAMP_STORE_APPS", value);
        }
        let out = command.output().unwrap();
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{switch:?}: {said}");
        assert!(said.contains("is off"), "{switch:?}: {said}");
        assert!(out.stdout.is_empty(), "{switch:?}");
    }
}

/// An install line is refused as off, and nothing is read, asked or run.
#[test]
fn an_install_with_the_switch_off_is_refused_and_nothing_runs() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    let (mut fx, mut skills) = (FakeEffects::default(), FakeCompanions::default());
    let mut yes = FakeYes::granting(vec![]);
    let reader = StubReader::new(AppPackage {
        app: AppSpec {
            name: "alpha".into(),
            title: "Alpha".into(),
            version: "1.0.0".into(),
            min_build: 1,
            net: false,
            logs: false,
            grants: vec![],
            entry: "app.main".into(),
        },
        files: BTreeMap::new(),
        skills: vec![],
    });
    let mut parts = Parts {
        layout: &layout,
        fx: &mut fx,
        skills: &mut skills,
        yes: &mut yes,
        reader: &reader,
        staged_owner: 0,
        build: Some(200),
    };
    let line = json!({"install": {"path": layout.staging.join("p.tar").display().to_string()}});
    assert_eq!(
        answer(false, &line.to_string(), &mut parts),
        json!({"refused": OFF})
    );
    assert!(fx.calls.is_empty(), "no effect ran");
    assert_eq!(yes.asked, 0, "no yes was taken");
    assert!(skills.placed.is_empty());
    assert!(!layout.code.exists(), "nothing was written");
}
