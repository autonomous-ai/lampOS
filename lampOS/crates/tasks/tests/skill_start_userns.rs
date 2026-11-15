//! `undumped_may` against the real kernel (#7133), with the caller in the
//! user namespaces OpenClaw puts its agents in: the gateway's `unshare
//! --map-root-user`, and inside it the wrapper's `unshare --user
//! --map-user=<outer uid>`, whose root is not mapped (c0a4). The kernel
//! gives a non-dumpable task's `/proc/<pid>/status` to uid 0 there, and a
//! dumpable one's to its euid; the `/proc/<pid>` folder stays at the euid
//! either way. A fake `Peers` can't show this. Must run as a non-root user;
//! skips, saying why, when user namespaces are not permitted. The traced
//! case needs `strace`, and is skipped without it.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use lamp_tasks::skill_start::{Kernel, undumped_may};

/// Set in the environment of this test binary when it is re-run as the
/// caller: "0" non-dumpable, "1" dumpable.
const ROLE: &str = "LAMP_TASKS_USERNS_CALLER";

/// The caller, when this binary runs as one: sets its dumpable flag, says
/// so on stdout, then waits to be looked at. Run as a plain test it does
/// nothing.
#[test]
fn userns_caller() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    if role == "0" {
        rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
            .expect("PR_SET_DUMPABLE 0");
    }
    println!("ready");
    std::thread::sleep(std::time::Duration::from_secs(10));
}

/// Why this test can't mean anything here, if it can't.
fn skip_reason() -> Option<String> {
    if rustix::process::getuid().is_root() {
        return Some("running as root: the caller must be a non-root user".into());
    }
    match Command::new("unshare")
        .args(["--map-root-user", "true"])
        .stderr(Stdio::piped())
        .output()
    {
        Ok(out) if out.status.success() => None,
        Ok(out) => Some(format!(
            "unshare --map-root-user is not permitted: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(err) => Some(format!("no unshare: {err}")),
    }
}

fn has_strace() -> bool {
    Command::new("strace")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The deepest process under `pid`: the caller, however many launchers
/// (strace, unshare) stand between.
fn leaf(pid: u32) -> u32 {
    let mut kids = Vec::new();
    if let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) {
        for task in tasks.flatten() {
            let text = std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
            kids.extend(
                text.split_whitespace()
                    .filter_map(|kid| kid.parse::<u32>().ok()),
            );
        }
    }
    kids.first().map_or(pid, |&kid| leaf(kid))
}

/// Where the caller runs.
#[derive(Clone, Copy, Debug)]
enum Place {
    /// This process's own user namespace (lamp-tasks').
    Own,
    /// A map-root user namespace of its own (`unshare --map-root-user`).
    MapRoot,
    /// OpenClaw's: inside a map-root namespace, a namespace of its own
    /// that maps the outer uid back and leaves root unmapped.
    OpenClaw,
}

/// A caller, in `place`, under strace (`traced`) or not, and its pid as
/// this process sees it.
fn caller(place: Place, dumpable: bool, traced: bool) -> (Child, u32) {
    let me = std::env::current_exe().expect("this test binary");
    let uid = rustix::process::getuid().as_raw();
    let gid = rustix::process::getgid().as_raw();
    let mut line: Vec<std::ffi::OsString> = Vec::new();
    if traced {
        line.extend(["strace", "-f", "-qq", "-o", "/dev/null"].map(Into::into));
    }
    if matches!(place, Place::MapRoot | Place::OpenClaw) {
        let unshare = [
            "unshare",
            "--map-root-user",
            "--mount",
            "--pid",
            "--fork",
            "--kill-child",
        ];
        line.extend(unshare.map(Into::into));
    }
    if matches!(place, Place::OpenClaw) {
        line.extend(["unshare", "--user"].map(Into::into));
        line.push(format!("--map-user={uid}").into());
        line.push(format!("--map-group={gid}").into());
    }
    line.push(me.into());
    line.extend(
        [
            "--exact",
            "userns_caller",
            "--nocapture",
            "--test-threads",
            "1",
        ]
        .map(Into::into),
    );
    let mut child = Command::new(&line[0])
        .args(&line[1..])
        .env(ROLE, if dumpable { "1" } else { "0" })
        .stdout(Stdio::piped())
        .spawn()
        .expect("the caller's launcher");
    let mut out = BufReader::new(child.stdout.take().expect("stdout"));
    let mut text = String::new();
    while !text.contains("ready") {
        text.clear();
        assert!(
            out.read_line(&mut text).expect("caller's stdout") > 0,
            "caller never got ready"
        );
    }
    let pid = leaf(child.id());
    (child, pid)
}

/// `undumped_may` on a caller made as asked.
fn check(place: Place, dumpable: bool, traced: bool) -> Result<(), String> {
    let (mut child, pid) = caller(place, dumpable, traced);
    let owner = |path: String| {
        std::fs::metadata(path).map(|meta| std::os::unix::fs::MetadataExt::uid(&meta))
    };
    let folder = owner(format!("/proc/{pid}"));
    let status = owner(format!("/proc/{pid}/status"));
    let got = undumped_may(&Kernel, pid);
    let _ = child.kill();
    let _ = child.wait();
    println!(
        "{place:?} dumpable={dumpable} traced={traced}: /proc {folder:?}, status {status:?}, {got:?}"
    );
    got
}

#[test]
fn a_launcher_is_judged_by_its_status_owner_on_the_real_kernel() {
    if let Some(reason) = skip_reason() {
        println!("skipped: {reason}");
        return;
    }
    let dumpable = Err("the caller can still be traced (it is dumpable)".to_owned());
    // OpenClaw's namespaces, as on c0a4: non-dumpable is taken, dumpable
    // is refused (a real check there).
    assert_eq!(check(Place::OpenClaw, false, false), Ok(()));
    assert_eq!(check(Place::OpenClaw, true, false), dumpable);
    if has_strace() {
        assert_eq!(
            check(Place::OpenClaw, false, true),
            Err("the caller is being traced".into())
        );
    } else {
        println!("traced case skipped: no strace");
    }
    // lamp-tasks' own namespace: the same.
    assert_eq!(check(Place::Own, false, false), Ok(()));
    assert_eq!(check(Place::Own, true, false), dumpable);
    // A map-root namespace: non-dumpable is taken; there a dumpable
    // caller's owner is the same uid, so it can't be told apart.
    assert_eq!(check(Place::MapRoot, false, false), Ok(()));
}
