use lamp_apps::LAUNCHER;
use std::fs;
use std::process::Command;

fn python() -> bool {
    Command::new("python3").arg("--version").output().is_ok()
}

fn run(
    root: &std::path::Path,
    launcher: &std::path::Path,
    name: &str,
    entry: &str,
) -> (Option<i32>, String, String) {
    let out = Command::new("python3")
        .arg("-I")
        .arg(launcher)
        .env_clear()
        .env("LAMP_APPS_ROOT", root)
        .env("LAMP_APP_NAME", name)
        .env("ENTRY", entry)
        .current_dir(root)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_launcher_runs_the_entry_from_the_apps_src_under_dash_i() {
    if !python() {
        eprintln!("python3 is not here: launcher test skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let launcher = dir.path().join("run.py");
    fs::write(&launcher, LAUNCHER).unwrap();
    let src = dir.path().join("apps/x/current/src/pkg");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("__init__.py"), "").unwrap();
    fs::write(
        src.join("app.py"),
        "import sys\nprint('ran', __name__, sys.argv[0] != '')\n",
    )
    .unwrap();
    let root = dir.path().join("apps");
    let (code, out, err) = run(&root, &launcher, "x", "pkg.app");
    assert_eq!(code, Some(0), "{err}");
    assert_eq!(out.trim(), "ran __main__ True");
}

#[test]
fn the_launcher_refuses_what_the_helper_would_refuse_with_exit_78() {
    if !python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let launcher = dir.path().join("run.py");
    fs::write(&launcher, LAUNCHER).unwrap();
    let root = dir.path().join("apps");
    fs::create_dir_all(root.join("x/current/src")).unwrap();
    fs::write(root.join("x/current/src/ok.py"), "print('no')\n").unwrap();
    for (name, entry) in [
        ("x", "../ok"),
        ("x", "a..b"),
        ("x", ".ok"),
        ("x", "ok."),
        ("x", ""),
        ("x", "OK"),
        ("x", "a/b"),
        ("../x", "ok"),
        ("X", "ok"),
        ("", "ok"),
        ("x", "missing"),
        ("x", "ok;x"),
    ] {
        let (code, out, err) = run(&root, &launcher, name, entry);
        assert_eq!(code, Some(78), "{name:?} {entry:?}: {err}");
        assert!(out.is_empty(), "{name:?} {entry:?} ran: {out}");
    }
}

#[test]
fn a_crash_inside_the_app_is_not_a_refusal() {
    if !python() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let launcher = dir.path().join("run.py");
    fs::write(&launcher, LAUNCHER).unwrap();
    let root = dir.path().join("apps");
    fs::create_dir_all(root.join("x/current/src")).unwrap();
    fs::write(
        root.join("x/current/src/boom.py"),
        "raise RuntimeError('boom')\n",
    )
    .unwrap();
    let (code, _, err) = run(&root, &launcher, "x", "boom");
    assert_eq!(code, Some(1), "{err}");
}
