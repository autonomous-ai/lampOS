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

#[test]
fn the_helper_puts_its_launcher_in_place_once_and_replaces_a_changed_one() {
    use lamp_apps::{LAUNCHER, launcher::ensure, layout::Layout};
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let layout = Layout::under(root.path());
    ensure(&layout).unwrap();
    let path = layout.launcher();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LAUNCHER);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    std::fs::write(&path, "tampered").unwrap();
    ensure(&layout).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LAUNCHER);
    assert!(!layout.code.join(".run.py.new").exists());
    // It is no app: the list does not see it.
    assert!(lamp_apps::manage::list(&layout).is_empty());
}

#[test]
fn the_name_is_the_units_argument_and_a_bad_one_exits_78() {
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().join("run.py");
    std::fs::write(&run, lamp_apps::LAUNCHER).unwrap();
    let status = std::process::Command::new("python3")
        .arg("-I")
        .arg(&run)
        .arg("Not A Name")
        .env_clear()
        .env("ENTRY", "app.main")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(78));
}

/// Whether `ip` is inside `net` (`a.b.c.d/n` or an IPv6 `x::/n`).
fn within(ip: std::net::IpAddr, net: &str) -> bool {
    use std::net::IpAddr;
    let (base, bits) = net.split_once('/').unwrap();
    let (base, bits): (IpAddr, u32) = (base.parse().unwrap(), bits.parse().unwrap());
    let wide = |ip: IpAddr| match ip {
        IpAddr::V4(v4) => (u128::from(u32::from(v4)), 32u32),
        IpAddr::V6(v6) => (u128::from(v6), 128u32),
    };
    let ((a, width), (b, other)) = (wide(ip), wide(base));
    width == other && (a >> (width - bits)) == (b >> (width - bits))
}

/// The net unit drops every private, loopback and link-local range
/// (`IPAddressDeny`), so a resolver inside one answers nothing and one that
/// is the router's would open its admin page to the app: every nameserver the
/// helper hands a net app is public by address, checked against the unit's own
/// list. Nothing else but comments and `options` is in the file.
#[test]
fn the_net_resolver_is_public_by_address_against_the_net_units_own_deny_list() {
    let unit = fs::read_to_string(format!(
        "{}/../../deploy/product/lampapp-net@.service",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let deny: Vec<&str> = unit
        .lines()
        .find_map(|line| line.strip_prefix("IPAddressDeny="))
        .unwrap()
        .split_whitespace()
        .collect();
    assert!(deny.len() >= 10, "{deny:?}");
    let mut servers = 0;
    for line in lamp_apps::RESOLV.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with("options ") {
            continue;
        }
        let ip: std::net::IpAddr = line
            .strip_prefix("nameserver ")
            .unwrap_or_else(|| panic!("not a nameserver line: {line:?}"))
            .parse()
            .unwrap();
        for net in &deny {
            assert!(!within(ip, net), "{ip} is inside the denied {net}");
        }
        servers += 1;
    }
    assert!(servers >= 2, "a resolver with no spare is a single point");
}

#[test]
fn the_helper_writes_the_resolver_beside_the_launcher_and_again_when_it_differs() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let layout = lamp_apps::layout::Layout::under(dir.path());
    lamp_apps::launcher::ensure(&layout).unwrap();
    let path = layout.resolver();
    assert_eq!(fs::read_to_string(&path).unwrap(), lamp_apps::RESOLV);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(fs::read_to_string(layout.launcher()).unwrap(), LAUNCHER);
    // Another file there is replaced by this build's.
    fs::write(&path, "nameserver 192.168.1.1\n").unwrap();
    lamp_apps::launcher::ensure(&layout).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), lamp_apps::RESOLV);
    // No temporary file is left, and neither name is an app.
    assert!(!layout.code.join(".resolv.conf.new").exists());
}
