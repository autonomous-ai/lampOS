//! `undumped_may` against the real kernel (#7133), with the caller in the
//! user namespaces OpenClaw puts its agents in: the gateway's `unshare
//! --map-root-user`, and inside it the wrapper's `unshare --user
//! --map-user=<outer uid>`, whose root is not mapped (c0a4). The kernel
//! gives a non-dumpable task's `/proc/<pid>/status` to uid 0 there, and a
//! dumpable one's to its euid; the `/proc/<pid>` folder stays at the euid
//! either way. A fake `Peers` can't show this. Must run as a non-root user;
//! skips, saying why, when user namespaces are not permitted. The traced
//! case needs `strace`, and is skipped without it.
//!
//! And #7024: lamp-tasks' side (this process) starts a run's fence itself,
//! outside a "gateway" made as OpenClaw's (`unshare --map-root-user
//! --mount --pid --fork --mount-proc`) in which the launcher runs; from
//! inside that gateway the script's pid is not there at all.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use lamp_tasks::skill_start::{FileSeen, Grant, Kernel, Peer, Peers, Server, Starts, undumped_may};

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

/// Set when this binary is re-run as the launcher in the gateway: the
/// socket's path and the run's id, `<socket>\n<id>`.
const LAUNCHER_ROLE: &str = "LAMP_TASKS_USERNS_LAUNCHER";
/// Set when this binary is re-run as the gateway's probe: a file holding
/// the script's host pid, its script's path, and the file the probe's
/// findings go to, one per line (never in any gateway process's
/// arguments, which the probe looks through for the script's path).
const PROBE_ROLE: &str = "LAMP_TASKS_USERNS_PROBE";

/// The launcher, when this binary runs as one in the gateway: claims the
/// run and relays it, exiting with the script's code.
#[test]
fn userns_launcher() {
    let Ok(said) = std::env::var(LAUNCHER_ROLE) else {
        return;
    };
    let (socket, id) = said.split_once('\n').expect("socket and id");
    let (code, why) = lamp_tasks::skill_start::launch_at(Path::new(socket), id);
    if !why.is_empty() {
        eprintln!("skill-start: {why}");
    }
    std::process::exit(code);
}

/// The probe, in the gateway beside the launcher: what it can find of the
/// script, written to its findings file as `name=value` lines.
#[test]
fn userns_probe() {
    let Ok(input) = std::env::var(PROBE_ROLE) else {
        return;
    };
    let said = std::fs::read_to_string(input).unwrap();
    let mut lines = said.lines();
    let host_pid: i32 = lines.next().unwrap().parse().unwrap();
    let script = lines.next().unwrap().to_owned();
    let findings = lines.next().unwrap().to_owned();
    // Any process here whose arguments name the script.
    let seen = std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().parse::<u32>().is_ok())
        .any(|entry| {
            std::fs::read(entry.path().join("cmdline"))
                .is_ok_and(|line| String::from_utf8_lossy(&line).contains(&script))
        });
    let environ = std::fs::read(format!("/proc/{host_pid}/environ")).is_ok();
    // `kill(pid, 0)`: the pid named at all (ptrace and pidfd_open name it
    // the same way).
    let named = rustix::process::Pid::from_raw(host_pid)
        .is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok());
    let traced = has_strace()
        && Command::new("strace")
            .args([
                "-qq",
                "-o",
                "/dev/null",
                "-p",
                &host_pid.to_string(),
                "-e",
                "none",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    std::fs::write(
        findings,
        format!("seen={seen}\nenviron={environ}\nnamed={named}\ntraced={traced}\n"),
    )
    .unwrap();
}

/// lamp-tasks' view of a launcher here: the kernel's for its pid, uid,
/// lines and dumpable flag; its program taken as the launcher (a test
/// can't make a root-owned copy of itself).
struct TestLauncher;

fn launcher_seen() -> FileSeen {
    FileSeen {
        file: true,
        uid: 0,
        mode: 0o100755,
        dev: 1,
        ino: 1,
    }
}

impl Peers for TestLauncher {
    fn peer(&self, stream: &std::os::unix::net::UnixStream) -> Option<Peer> {
        Kernel.peer(stream)
    }
    fn exe_link(&self, _: u32) -> std::io::Result<PathBuf> {
        Ok(PathBuf::from("/test/skill-start"))
    }
    fn exe_file(&self, _: u32) -> std::io::Result<FileSeen> {
        Ok(launcher_seen())
    }
    fn file(&self, _: &Path) -> std::io::Result<FileSeen> {
        Ok(launcher_seen())
    }
    fn environ(&self, _: u32) -> std::io::Result<Vec<u8>> {
        Ok(Vec::new())
    }
    fn maps(&self, _: u32) -> std::io::Result<String> {
        Ok(String::new())
    }
    fn status(&self, pid: u32) -> std::io::Result<String> {
        Kernel.status(pid)
    }
    fn proc_owner(&self, pid: u32) -> std::io::Result<u32> {
        Kernel.proc_owner(pid)
    }
    fn user_ns_root(&self, pid: u32) -> std::io::Result<Option<u32>> {
        Kernel.user_ns_root(pid)
    }
    fn read_line(
        &self,
        stream: &std::os::unix::net::UnixStream,
        most: usize,
    ) -> std::io::Result<(String, Option<u32>)> {
        Kernel.read_line(stream, most)
    }
}

/// The host pid of the process whose arguments are exactly `argv`.
fn pid_of(argv: &[&str]) -> Option<u32> {
    let want: Vec<u8> = argv
        .iter()
        .flat_map(|arg| [arg.as_bytes(), b"\0"].concat())
        .collect();
    std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .find_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            (std::fs::read(entry.path().join("cmdline")).ok()? == want).then_some(pid)
        })
}

/// Waits for `path` to be there, at most 20 s.
fn wait_for(path: &Path) {
    let until = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(Instant::now() < until, "{} never came", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// #7024 on the real kernel: lamp-tasks (this process) serves the launcher
/// socket and starts the run's fence itself with the real `fence_args`; the
/// launcher runs in a gateway made as OpenClaw's. The script got its
/// token on fd 3; from inside the gateway no process names the script, its
/// host pid is not there (`kill`, `environ` and, with strace, `ptrace` all
/// fail); its output and exit code reach the gateway through the launcher.
#[test]
fn the_script_runs_outside_the_gateway_and_no_gateway_process_sees_it() {
    if let Some(reason) = skip_reason() {
        println!("skipped: {reason}");
        return;
    }
    // Not under /tmp: the fence puts a tmpfs of its own over /tmp.
    let base = tempfile::Builder::new()
        .prefix("ss7024")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let base_path = base.path().to_owned();
    let start = base_path.join("start");
    std::fs::create_dir(&start).unwrap();
    std::fs::set_permissions(&start, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let socket = start.join("start.sock");
    let run = base_path.join("project/skill-task-1");
    let skill = base_path.join("skill");
    let hidden = base_path.join("hidden");
    for folder in [&run, &skill, &hidden] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let script = skill.join("main.sh");
    std::fs::write(
        &script,
        "printf '%s' \"$LAMP_CONTROL_TOKEN\" > \"$HOME/token\"\n\
         echo script-ran\n\
         touch \"$HOME/up\"\n\
         while [ ! -e \"$HOME/go\" ]; do sleep 0.05; done\n\
         exit 7\n",
    )
    .unwrap();
    let token = "secret-7024";
    let id = lamp_tasks::skill_task::run_id();
    let starts = Starts::default();
    let grant = Grant {
        script: script.clone(),
        interpreter: "/bin/sh".into(),
        skill: skill.clone(),
        run: run.clone(),
        socket: "/run/lampos-control/control.sock".into(),
        pythonpath: "/sdk".into(),
    };
    let live = starts.open(&id, grant, token).unwrap();
    let fence_hidden = hidden.clone();
    let server = Server {
        socket: socket.clone(),
        starts: starts.clone(),
        peers: Arc::new(TestLauncher),
        own_uid: rustix::process::getuid().as_raw(),
        launcher: PathBuf::from("/test/skill-start"),
        started_within: Duration::from_secs(30),
        fence: Arc::new(move |grant: &Grant| {
            let mut fence = Command::new("unshare");
            fence
                .args(lamp_tasks::gateway::fence_args(
                    grant,
                    rustix::process::getuid().as_raw(),
                    rustix::process::getgid().as_raw(),
                    &[fence_hidden.as_path()],
                ))
                .env_clear()
                .env("PATH", lamp_tasks::gateway::SCRIPT_PATH)
                .current_dir("/");
            fence
        }),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let serving = server.start(stop.clone()).unwrap();
    wait_for(&socket);
    // The gateway: the launcher, and beside it the probe once the script's
    // host pid is known; it ends with the launcher's code.
    let me = std::env::current_exe().unwrap();
    let pid_file = base_path.join("hostpid");
    let findings = base_path.join("findings");
    let gateway = Command::new("unshare")
        .args([
            "--map-root-user",
            "--mount",
            "--pid",
            "--fork",
            "--kill-child",
            "--mount-proc",
            "/bin/sh",
            "-c",
            r#"env "$2=$3" "$0" --exact userns_launcher --nocapture --test-threads 1 > "$4/out" & l=$!
               while [ ! -e "$4/hostpid" ]; do sleep 0.05; done
               env "$5=$4/hostpid" "$0" --exact userns_probe --nocapture --test-threads 1 > /dev/null
               touch "$4/project/skill-task-1/go"
               wait $l"#,
        ])
        .arg(&me)
        .arg(LAUNCHER_ROLE)
        .arg(format!("{}\n{id}", socket.display()))
        .arg(&base_path)
        .arg(PROBE_ROLE)
        .spawn()
        .unwrap();
    wait_for(&run.join("up"));
    let interpreter_argv = ["/bin/sh", script.to_str().unwrap()];
    let host_pid = pid_of(&interpreter_argv).expect("the script's host pid");
    // From the host (lamp-tasks' side) the script is there, its token in
    // its environment: the pid is the right one.
    let environ = std::fs::read(format!("/proc/{host_pid}/environ")).unwrap_or_default();
    assert!(
        String::from_utf8_lossy(&environ).contains(token),
        "the script's own environment"
    );
    // Whole before it is there: written aside, then renamed.
    let aside = base_path.join("hostpid.new");
    std::fs::write(
        &aside,
        format!("{host_pid}\n{}\n{}\n", script.display(), findings.display()),
    )
    .unwrap();
    std::fs::rename(&aside, &pid_file).unwrap();
    let mut gateway = gateway;
    let status = gateway.wait().unwrap();
    stop.store(true, std::sync::atomic::Ordering::Release);
    serving.join().unwrap();
    let found = std::fs::read_to_string(&findings).unwrap();
    println!("from inside the gateway: {found}");
    assert!(found.contains("seen=false\n"), "{found}");
    assert!(found.contains("environ=false\n"), "{found}");
    assert!(found.contains("traced=false\n"), "{found}");
    // The gateway's own pids are few and small: a host pid that large is
    // not one of them.
    if host_pid > 1000 {
        assert!(found.contains("named=false\n"), "{found}");
    }
    assert_eq!(std::fs::read_to_string(run.join("token")).unwrap(), token);
    assert_eq!(status.code(), Some(7), "the script's code, relayed");
    // The launcher's stdout: the test harness's own lines, and the
    // script's, relayed.
    let out = std::fs::read_to_string(base_path.join("out")).unwrap_or_default();
    assert!(out.contains("script-ran\n"), "{out}");
    assert_eq!(live.ran(), Ok(()));
}
