//! `lamp-cloud` is off unless `LAMP_CLOUD=on` (the reviewer on #1418): the
//! binary itself, with a complete account aimed at a local listener, says
//! it is off and connects nowhere without the switch, and connects with it.
//! The product's unit turns it on (#1815), and stops V1's `os-server` only as
//! it really starts, never through `Conflicts=` (#1909).
use std::io::{ErrorKind, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_lamp-cloud");
const UNIT: &str = include_str!("../../../../deploy/product/lamp-cloud.service");
const TMPFILES: &str = include_str!("../../../../deploy/product/lampos.conf");

/// A `CREDENTIALS_DIRECTORY` as the unit's `LoadCredential=` makes it, with
/// the broker at `port` on this host. No backend base, so no ping is made.
fn credentials(name: &str, port: u16) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/lcw-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let account = serde_json::json!({
        "device_id": "dev-42", "mqtt_endpoint": "127.0.0.1", "mqtt_port": port,
        "mqtt_username": "lamp-dev-42", "mqtt_password": "made-up-pw",
        "fa_channel": "fa/dev-42", "fd_channel": "fd/dev-42", "llm_base_url": ""
    });
    std::fs::write(dir.join("account"), account.to_string()).unwrap();
    std::fs::write(dir.join("key"), "made-up-key\n").unwrap();
    std::fs::write(dir.join("build"), "{}").unwrap();
    dir
}

fn spawn(credentials: &Path, switch: Option<&str>) -> Child {
    let mut command = Command::new(BIN);
    command
        .env("CREDENTIALS_DIRECTORY", credentials)
        .env_remove("STATE_DIRECTORY")
        .env_remove("LAMP_CLOUD")
        .env_remove("LAMP_BUS_ROOT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(value) = switch {
        command.env("LAMP_CLOUD", value);
    }
    command.spawn().unwrap()
}

/// The child's exit within `wait`, or `None` (it is then killed), and what
/// it said.
fn finish(mut child: Child, wait: Duration) -> (Option<ExitStatus>, String) {
    let until = Instant::now() + wait;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut said = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut said);
    }
    (status, said)
}

/// Whether the listener is connected to within `wait`.
fn connected(listener: &TcpListener, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok(_) => return true,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => panic!("{error}"),
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn without_the_switch_it_says_so_and_connects_nowhere() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let dir = credentials("off", listener.local_addr().unwrap().port());
    for switch in [None, Some("1"), Some("off"), Some("")] {
        let (status, said) = finish(spawn(&dir, switch), Duration::from_secs(10));
        let status =
            status.unwrap_or_else(|| panic!("{switch:?}: still running after 10 s: {said}"));
        assert!(status.success(), "{switch:?}: {status:?}");
        assert_eq!(
            said, "lamp-cloud: off: LAMP_CLOUD is not on\n",
            "{switch:?}"
        );
    }
    assert!(!connected(&listener, Duration::from_millis(300)));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn with_the_switch_on_it_connects_to_the_accounts_broker() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let dir = credentials("on", listener.local_addr().unwrap().port());
    let child = spawn(&dir, Some("on"));
    let reached = connected(&listener, Duration::from_secs(10));
    let (status, said) = finish(child, Duration::ZERO);
    assert!(reached, "no connection; it said: {said}");
    assert!(status.is_none(), "it ended ({status:?}); it said: {said}");
    assert!(!said.contains("not on"), "{said}");
    // The broker's password and the key are never said.
    assert!(
        !said.contains("made-up-pw") && !said.contains("made-up-key"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_product_unit_is_on_and_stops_v1s_os_server_only_as_it_really_starts() {
    // #1815: on in the product, so a Lamp 2 set up from the app shows online
    // at once and after a reboot, starting with the lamp once setup is done.
    let lines: Vec<&str> = UNIT
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .collect();
    for line in [
        "Environment=LAMP_CLOUD=on",
        // #2636: the lamp's will, so its record shows it offline about 30 s
        // after its connection is lost.
        "Environment=LAMP_CLOUD_OFFLINE_WILL=on",
        // Restart and shutdown from the app, so a product lamp's `can`
        // names both and the app shows both buttons (#2193, #3697).
        "Environment=LAMP_APP_RESTART=on",
        "Environment=LAMP_SHUTDOWN=on",
        "PartOf=lampos.target",
        "WantedBy=lampos.target",
        "ConditionPathExists=/var/lib/lampos-setup/done",
        "ConditionPathExists=/etc/lampOS/setup/backend.json",
        // V1's os-server holds the same client id: stopped as root, only
        // once the conditions have held, and a lamp without it goes on.
        "ExecStartPre=-+/usr/bin/systemctl stop os-server.service",
    ] {
        assert!(lines.contains(&line), "{line}");
    }
    // Never Conflicts=: its stop is queued before the conditions are
    // checked, so a lamp-cloud skipped for want of setup's account would
    // still stop os-server (the reviewer on #1909; the rule of #1418).
    assert!(
        !lines.iter().any(|line| line.starts_with("Conflicts=")),
        "a skipped lamp-cloud would stop os-server"
    );
    assert!(UNIT.contains("systemctl mask lamp-cloud"));
}

#[test]
fn lamp_4aces_cloud_on_writes_the_units_whole_drop_in() {
    // lamp-4ace's own installer (#1420) writes its drop-in whole: the switch
    // and the conflict, as it did before the product's unit was on (#1815).
    const CLOUD_ON: &str = include_str!("../../../../deploy/lamp-4ace/cloud-on.sh");
    let line = CLOUD_ON
        .lines()
        .find(|line| line.contains("> '$DROPIN'"))
        .expect("cloud-on.sh writes the drop-in");
    assert!(
        line.contains(
            r"[Unit]\nConflicts=os-server.service\n[Service]\nEnvironment=LAMP_CLOUD=on\n"
        ),
        "{line}"
    );
}

/// Reviewer-lead's P3 on #2273: the photo folder lamp-cloud's unit may
/// write is the one the product's tmpfiles makes at boot, `lampos`'s at
/// 0700 as the eyes require, and no unit's own runtime folder.
#[test]
fn the_photo_folder_the_unit_may_write_is_the_one_made_at_boot() {
    let writable: Vec<&str> = UNIT
        .lines()
        .filter_map(|line| line.strip_prefix("ReadWritePaths="))
        .flat_map(str::split_whitespace)
        .collect();
    assert!(writable.contains(&"-/run/lampos-photos"), "{writable:?}");
    let made: Vec<Vec<&str>> = TMPFILES
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|fields| fields.get(1) == Some(&"/run/lampos-photos"))
        .collect();
    assert_eq!(
        made,
        [["d", "/run/lampos-photos", "0700", "lampos", "lampos", "-"]]
    );
    assert!(!UNIT.contains("RuntimeDirectory=lampos-photos"));
}

/// #2562: lamp-cloud's unit may reach lamp-skills' call socket, for guard
/// mode from the app, where lamp-skills' unit makes it (its runtime folder,
/// given as its `--tools-dir`); a lamp without it still starts (`-`).
#[test]
fn the_unit_may_reach_lamp_skills_call_socket_where_lamp_skills_makes_it() {
    const SKILLS: &str = include_str!("../../../../deploy/product/lamp-skills.service");
    let writable: Vec<&str> = UNIT
        .lines()
        .filter_map(|line| line.strip_prefix("ReadWritePaths="))
        .flat_map(str::split_whitespace)
        .collect();
    assert!(writable.contains(&"-/run/lampos-skills"), "{writable:?}");
    assert!(SKILLS.contains("RuntimeDirectory=lampos-skills\n"));
    assert!(SKILLS.contains("--tools-dir /run/lampos-skills "));
    assert_eq!(lamp_cloud::guard::SOCKET, "/run/lampos-skills/call.sock");
}

/// #2813: lamp-cloud's unit may reach lamp-voice's app-make socket, in the
/// folder lamp-voice makes for it; a lamp without it still starts (`-`).
#[test]
fn the_unit_may_reach_lamp_voices_app_make_socket() {
    let writable: Vec<&str> = UNIT
        .lines()
        .filter_map(|line| line.strip_prefix("ReadWritePaths="))
        .flat_map(str::split_whitespace)
        .collect();
    assert!(writable.contains(&"-/run/lampos-app-make"), "{writable:?}");
    assert_eq!(
        std::path::Path::new(lamp_cloud::make_skill::SOCKET).parent(),
        Some(std::path::Path::new("/run/lampos-app-make"))
    );
}
