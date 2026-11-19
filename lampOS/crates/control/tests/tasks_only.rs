//! `lamp-control serve` with a skill's tasks on and the door off (#6059, the
//! first builder's (b)): the owner's side (`--admin-socket`) is served and
//! nothing listens on the network; with `LAMP_CONTROL=on` the door listens as
//! before; with both off it says so and exits 0.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A folder of the test's own, short enough for a socket path on the Mac
/// (103 bytes at most).
fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/lc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A port nothing listens on now.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `serve` with only the test's own paths, the switches as given.
fn serve(dir: &Path, port: u16, door: bool, tasks: bool) -> Child {
    serve_with(dir, port, door, tasks, false)
}

/// [`serve`], with the apps' socket too when `apps` (`LAMP_STORE_APPS=on`).
fn serve_with(dir: &Path, port: u16, door: bool, tasks: bool, apps: bool) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-control"));
    command
        .args([
            "serve",
            "--name",
            "test-lamp",
            "--sight-file",
            "off",
            "--frames-socket",
            "off",
        ])
        .arg("--state-dir")
        .arg(dir.join("state"))
        .arg("--admin-socket")
        .arg(dir.join("admin/admin.sock"))
        // A run user every Mac and Linux names in /etc/passwd: the run
        // socket is bound, and a mint meets the lamp's (missing) yes.
        .args(["--run-socket-user", "root"])
        .arg("--run-socket")
        .arg(dir.join("admin/run.sock"))
        .args(["--listen", &format!("127.0.0.1:{port}")])
        .env_remove("LAMP_CONTROL")
        .env_remove("LAMP_SKILL_TASKS")
        .env_remove("LAMP_CONTROL_RUN_SOCKET_USER")
        .env_remove("LAMP_CONTROL_LOCAL_AGENT")
        .env_remove("LAMP_STORE_APPS")
        .arg("--app-socket")
        .arg(dir.join("admin/apps.sock"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if door {
        command.env("LAMP_CONTROL", "on");
    }
    if tasks {
        command.env("LAMP_SKILL_TASKS", "on");
    }
    if apps {
        command.env("LAMP_STORE_APPS", "on");
    }
    command.spawn().unwrap()
}

/// Waits up to 10 s for `ready`.
fn wait_for(mut ready: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// One line asked on the owner's socket, one answered.
fn ask(admin: &Path, line: &str) -> String {
    let mut stream = UnixStream::connect(admin).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    writeln!(stream, "{line}").unwrap();
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).unwrap();
    answer
}

fn stop(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn tasks_on_and_the_door_off_serves_the_owners_side_and_nothing_on_the_network() {
    let dir = folder("tasks");
    let port = free_port();
    let child = serve(&dir, port, false, true);
    let admin = dir.join("admin/admin.sock");
    let up = wait_for(|| UnixStream::connect(&admin).is_ok());
    let listening = TcpStream::connect(("127.0.0.1", port)).is_ok();
    let run_socket = wait_for(|| dir.join("admin/run.sock").exists());
    let mint = if up {
        ask(
            &admin,
            r#"{"mint": {"skill": "photo-booth", "run": "task-1"}}"#,
        )
    } else {
        String::new()
    };
    stop(child);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(up, "the owner's side is served with a skill's tasks on");
    assert!(
        !listening,
        "nothing listens on the network without the door"
    );
    assert!(run_socket, "the run socket is served without the door");
    // No yes is kept for this skill: the mint is refused in words, no token.
    // The words are the yes source's own (`NoYes` today, #6509's `KeptYes`
    // next), so only the refusal's shape is held here.
    assert!(mint.contains("\"error\""), "{mint}");
    assert!(!mint.contains("\"token\""), "{mint}");
}

#[test]
fn the_door_on_listens_as_before() {
    let dir = folder("door");
    let port = free_port();
    let child = serve(&dir, port, true, false);
    let admin = dir.join("admin/admin.sock");
    let up = wait_for(|| UnixStream::connect(&admin).is_ok());
    let listening = wait_for(|| TcpStream::connect(("127.0.0.1", port)).is_ok());
    stop(child);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(up);
    assert!(listening, "LAMP_CONTROL=on listens on --listen as before");
}

#[test]
fn both_off_says_so_and_exits_0_with_nothing_served() {
    let dir = folder("off");
    let port = free_port();
    let child = serve(&dir, port, false, false);
    let out = child.wait_with_output().unwrap();
    let served = dir.join("admin/admin.sock").exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing listens"));
    assert!(!served);
}

/// Developer access turned Off (#6401): the change helper removes the drop-in
/// and RESTARTS lamp-control (it used to stop it). The restarted lamp-control
/// is the same program without `LAMP_CONTROL=on`: no computer can reach the
/// lamp (the network port refuses, a connection held before is gone with the
/// old process, no pairing code is made), while the owner's side, the run
/// socket and the apps' socket keep serving.
#[test]
fn restarted_with_developer_access_off_no_computer_reaches_the_lamp_and_the_owners_side_serves() {
    let dir = folder("restart-off");
    let port = free_port();
    let admin = dir.join("admin/admin.sock");
    // Developer access on: the door listens, a code can be made, and a
    // computer's connection is open.
    let on = serve_with(&dir, port, true, true, true);
    assert!(wait_for(|| UnixStream::connect(&admin).is_ok()));
    assert!(wait_for(|| TcpStream::connect(("127.0.0.1", port)).is_ok()));
    let made = ask(&admin, r#"{"code": {"grants": ["light"]}}"#);
    assert!(made.contains("\"code\""), "{made}");
    let mut held = TcpStream::connect(("127.0.0.1", port)).unwrap();
    held.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    // The helper's restart with the drop-in gone: the old process ends, the
    // new one starts with the door off.
    stop(on);
    let mut byte = [0u8; 1];
    let gone = std::io::Read::read(&mut held, &mut byte);
    assert!(
        matches!(gone, Ok(0) | Err(_)),
        "a connection held to the old door is dropped: {gone:?}"
    );
    let _ = std::fs::remove_file(&admin);
    let off = serve_with(&dir, port, false, true, true);
    let up = wait_for(|| UnixStream::connect(&admin).is_ok());
    let refused = TcpStream::connect(("127.0.0.1", port)).is_err();
    let clients = if up {
        ask(&admin, r#"{"clients": true}"#)
    } else {
        String::new()
    };
    let code = if up {
        ask(&admin, r#"{"code": {"grants": ["light"]}}"#)
    } else {
        String::new()
    };
    let mint = if up {
        ask(
            &admin,
            r#"{"mint": {"skill": "photo-booth", "run": "task-1"}}"#,
        )
    } else {
        String::new()
    };
    let run_socket = wait_for(|| dir.join("admin/run.sock").exists());
    let apps_socket = wait_for(|| dir.join("admin/apps.sock").exists());
    stop(off);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(up, "the owner's side is served after the restart");
    assert!(refused, "nothing listens on the network after Off");
    assert!(clients.contains("\"listening\":false"), "{clients}");
    assert!(
        code.contains("developer access is off on this lamp") && !code.contains("\"code\":"),
        "no pairing code is made with the door off: {code}"
    );
    // The owner's side still answers (no yes is kept for this skill: refused in words).
    assert!(mint.contains("\"error\""), "{mint}");
    assert!(run_socket, "the run socket is served");
    assert!(apps_socket, "the apps' socket is served");
}
