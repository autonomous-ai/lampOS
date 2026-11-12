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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if door {
        command.env("LAMP_CONTROL", "on");
    }
    if tasks {
        command.env("LAMP_SKILL_TASKS", "on");
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
