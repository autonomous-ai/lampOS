//! `lamp-apps-manage run NAME` (#7956): what the two app templates start.
use lamp_apps::{layout::Layout, run::argv};
use std::process::Command;

#[test]
fn the_argv_is_the_fixed_launcher_with_the_checked_name() {
    let layout = Layout::product();
    assert_eq!(
        argv(&layout, "english-practice").unwrap(),
        [
            "/usr/bin/python3",
            "-I",
            "/opt/lampOS/apps/.run.py",
            "english-practice"
        ]
    );
}

#[test]
fn a_name_that_is_no_apps_is_refused() {
    let layout = Layout::product();
    for bad in [
        "",
        "../x",
        "A",
        "a b",
        "-I",
        "a/b",
        ".run.py",
        "x;y",
        "ab\n",
        &"a".repeat(80),
    ] {
        assert!(argv(&layout, bad).is_err(), "{bad:?}");
    }
}

/// The real binary: anything but exactly one valid name is refused with 78 (the unit's
/// `RestartPreventExitStatus`, so no restart loop), before any program is started.
#[test]
fn the_real_run_command_refuses_a_wrong_call_with_78() {
    for args in [
        vec!["run"],
        vec!["run", "../x"],
        vec!["run", "Bad"],
        vec!["run", "a", "b"],
        vec!["run", ""],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_lamp-apps-manage"))
            .args(&args)
            .env_clear()
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(78), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
    }
}
