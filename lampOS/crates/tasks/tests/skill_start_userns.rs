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
/// run and relays it, exiting with the script's code, as
/// `skill_start::launch_at` does, but saying the answer it was given on
/// stderr (the grant as JSON again: `parse_answer` refuses unknown fields,
/// so it is all the line carried), so the test can look for the token in
/// it.
#[test]
fn userns_launcher() {
    let Ok(said) = std::env::var(LAUNCHER_ROLE) else {
        return;
    };
    let (socket, id) = said.split_once('\n').expect("socket and id");
    let stream = std::os::unix::net::UnixStream::connect(socket).expect("lamp-tasks reached");
    stream
        .set_read_timeout(Some(lamp_tasks::skill_start::LINE_WITHIN))
        .unwrap();
    stream
        .set_write_timeout(Some(lamp_tasks::skill_start::LINE_WITHIN))
        .unwrap();
    let undumpable = || {
        rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
            .map_err(std::io::Error::from)
    };
    let grant = match lamp_tasks::skill_start::ask(&stream, id, undumpable) {
        Ok(grant) => grant,
        Err((code, why)) => {
            eprintln!("skill-start: {why}");
            std::process::exit(code);
        }
    };
    eprintln!("answer: {}", serde_json::to_string(&grant).unwrap());
    stream.set_read_timeout(None).unwrap();
    let (code, why) =
        lamp_tasks::skill_start::relay(&stream, std::io::stdout().lock(), std::io::stderr().lock());
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

/// The host pid of the process whose last argument is `script`: the
/// interpreter running it, whatever its options (so a fence that drops
/// `-s` still has its script found, and fails on what the script did, not
/// here). The fence's own `unshare` ends with the hidden paths, and the
/// steps between exec into the interpreter.
fn pid_of(script: &str) -> Option<u32> {
    let want = [script.as_bytes(), b"\0"].concat();
    std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .find_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            let line = std::fs::read(entry.path().join("cmdline")).ok()?;
            (line.ends_with(&want)
                && (line.len() == want.len() || line[line.len() - want.len() - 1] == 0))
                .then_some(pid)
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

/// What a run beside the gateway gave ([`run_beside_gateway`]).
struct Beside {
    /// The launcher's stdout: the test harness's own lines, and the
    /// script's, relayed.
    out: String,
    /// The launcher's stderr: its `answer:` line, and the script's stderr,
    /// relayed.
    err: String,
}

/// The token lamp-tasks gives the run; never anywhere the gateway reads.
const TOKEN: &str = "secret-7024";
/// What a lamp service's file under the `/run` stand-in holds.
const RUN_SECRET: &str = "run-file-7024";
/// What a settings file under the `/etc/lampOS` stand-in holds.
const ETC_SECRET: &str = "etc-file-7024";

/// #7024 on the real kernel: lamp-tasks (this process) serves the launcher
/// socket and starts the run's fence itself with the real `fence_args`, for
/// `script_text` in the skill's folder as `script_name`, run by
/// `interpreter`; the launcher runs in a gateway made as OpenClaw's.
/// `plant` is given the run's folder first, as a gateway agent could write
/// it. Stand-ins for `/run` (`<base>/run`: a lamp service's file, lamp-control's
/// socket and its owner socket beside it) and `/etc/lampOS`
/// (`<base>/etc-lampOS`), each holding a file this uid can read, are hidden
/// as the lamp's are; `@BASE@` in `script_text` is `<base>`. Checked here, for every run: the script got the token (in its own
/// environment, seen from the host); from inside the gateway no process
/// names the script, its host pid is not there (`kill`, `environ` and,
/// with strace, `ptrace` all fail); its code reaches the gateway through
/// the launcher; and the token is in none of what the gateway gets: the
/// launcher's stdout, its stderr, the answer it was given, or the run's
/// folder. The script must print, touch `up` in its working folder (the
/// run's), wait for `go` there, and exit 7.
fn run_beside_gateway(
    interpreter: &str,
    script_name: &str,
    script_text: &str,
    plant: impl FnOnce(&Path),
) -> Beside {
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
    let run_standin = base_path.join("run");
    let etc_standin = base_path.join("etc-lampOS");
    for folder in [
        &run,
        &skill,
        &hidden,
        &run_standin.join("lampos-seen"),
        &run_standin.join("lampos-control"),
        &etc_standin,
    ] {
        std::fs::create_dir_all(folder).unwrap();
    }
    for (file, text) in [
        (run_standin.join("lampos-seen/frame-link.env"), RUN_SECRET),
        (etc_standin.join("cloud.env"), ETC_SECRET),
    ] {
        std::fs::write(&file, text).unwrap();
        std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o644))
            .unwrap();
        // Readable here, by the uid the script runs as.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    }
    let control = run_standin.join("lampos-control/control.sock");
    let _control = std::os::unix::net::UnixListener::bind(&control).unwrap();
    let _admin =
        std::os::unix::net::UnixListener::bind(run_standin.join("lampos-control/admin.sock"))
            .unwrap();
    plant(&run);
    let script = skill.join(script_name);
    std::fs::write(
        &script,
        script_text.replace("@BASE@", base_path.to_str().unwrap()),
    )
    .unwrap();
    let id = lamp_tasks::skill_task::run_id();
    let starts = Starts::default();
    let grant = Grant {
        script: script.clone(),
        interpreter: interpreter.into(),
        skill: skill.clone(),
        run: run.clone(),
        socket: control.to_str().unwrap().into(),
        pythonpath: "/sdk".into(),
    };
    let live = starts.open(&id, grant, TOKEN).unwrap();
    let fence_hidden = [hidden.clone(), run_standin.clone(), etc_standin.clone()];
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
                    &fence_hidden
                        .iter()
                        .map(PathBuf::as_path)
                        .collect::<Vec<_>>(),
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
            // `sh -c <text> <me> <role> <said> <base> <probe role>`: $0..$4.
            r#"env "$1=$2" "$0" --exact userns_launcher --nocapture --test-threads 1 > "$3/out" 2> "$3/err" & l=$!
               while [ ! -e "$3/hostpid" ]; do sleep 0.05; done
               env "$4=$3/hostpid" "$0" --exact userns_probe --nocapture --test-threads 1 > /dev/null
               touch "$3/project/skill-task-1/go"
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
    let host_pid = pid_of(script.to_str().unwrap()).expect("the script's host pid");
    // From the host (lamp-tasks' side) the script is there, its token in
    // its environment: the pid is the right one, and the token reached it
    // (on fd 3: never written anywhere by this test).
    let environ = std::fs::read(format!("/proc/{host_pid}/environ")).unwrap_or_default();
    assert!(
        String::from_utf8_lossy(&environ).contains(&format!("LAMP_CONTROL_TOKEN={TOKEN}\0")),
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
    let out = std::fs::read_to_string(base_path.join("out")).unwrap_or_default();
    let err = std::fs::read_to_string(base_path.join("err")).unwrap_or_default();
    assert_eq!(
        status.code(),
        Some(7),
        "the script's code, relayed: {out}{err}"
    );
    assert_eq!(live.ran(), Ok(()));
    // The relay route: nothing the gateway gets carries the token.
    let answer = err
        .lines()
        .find_map(|line| line.strip_prefix("answer: "))
        .unwrap_or_else(|| panic!("the launcher's answer line: {err}"));
    assert!(answer.contains("\"interpreter\""), "{answer}");
    for (what, text) in [
        ("stdout", out.as_str()),
        ("stderr", err.as_str()),
        ("answer", answer),
    ] {
        assert!(
            !text.contains(TOKEN),
            "the token in the launcher's {what}: {text}"
        );
        for secret in [RUN_SECRET, ETC_SECRET] {
            assert!(
                !text.contains(secret),
                "{secret} in the launcher's {what}: {text}"
            );
        }
    }
    for entry in walk(&run) {
        let text = std::fs::read(&entry).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&text).contains(TOKEN),
            "the token in the run's folder: {}",
            entry.display()
        );
    }
    Beside { out, err }
}

/// Every file under `folder`, not following links.
fn walk(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut todo = vec![folder.to_owned()];
    while let Some(at) = todo.pop() {
        for entry in std::fs::read_dir(&at).into_iter().flatten().flatten() {
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => todo.push(entry.path()),
                Ok(kind) if kind.is_file() => files.push(entry.path()),
                _ => {}
            }
        }
    }
    files
}

/// #7024: the script runs outside the gateway, no gateway process sees
/// it, and its home is the fence's own, not the run's folder.
#[test]
fn the_script_runs_outside_the_gateway_and_no_gateway_process_sees_it() {
    if let Some(reason) = skip_reason() {
        println!("skipped: {reason}");
        return;
    }
    let ran = run_beside_gateway(
        "/bin/sh",
        "main.sh",
        "[ -n \"$LAMP_CONTROL_TOKEN\" ] && echo token-given\n\
         echo script-ran\n\
         echo \"home=$HOME\"\n\
         echo \"core=$(ulimit -c)\"\n\
         echo \"hardcore=$(ulimit -H -c)\"\n\
         touch up\n\
         while [ ! -e go ]; do sleep 0.05; done\n\
         exit 7\n",
        |_| {},
    );
    for line in [
        "script-ran\n",
        "token-given\n",
        "home=/tmp/lampos-home.",
        "core=0\n",
        "hardcore=0\n",
    ] {
        assert!(ran.out.contains(line), "{line}: {}", ran.out);
    }
}

/// The interpreter's `major.minor`, as the fence's `PATH` finds `python3`.
fn python_version() -> Option<String> {
    let out = Command::new("python3")
        .args(["-c", "import sys; print('%d.%d' % sys.version_info[:2])"])
        .env_clear()
        .env("PATH", lamp_tasks::gateway::SCRIPT_PATH)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// #7024's adversarial read: a gateway agent plants a `.pth` in the user
/// site-packages under the run's folder (the script's `HOME` before the
/// fix), printing the token to stdout and stderr, which the launcher
/// relays to the gateway. The Python script gets no user site (`-s`,
/// `PYTHONNOUSERSITE=1`) and a home of the fence's own, so the `.pth` never
/// runs and the token is not relayed.
#[test]
fn a_planted_pth_in_the_run_folder_never_relays_the_token() {
    if let Some(reason) = skip_reason() {
        println!("skipped: {reason}");
        return;
    }
    let Some(version) = python_version() else {
        println!(
            "skipped: no python3 on {}",
            lamp_tasks::gateway::SCRIPT_PATH
        );
        return;
    };
    let ran = run_beside_gateway(
        "python3",
        "main.py",
        "import os, sys, time\n\
         print('script-ran')\n\
         print('token-given' if os.environ.get('LAMP_CONTROL_TOKEN') else 'no-token')\n\
         print('home=' + os.environ.get('HOME', ''))\n\
         print('no_user_site=%d' % sys.flags.no_user_site)\n\
         import socket\n\
         c = socket.socket(socket.AF_UNIX)\n\
         c.connect(os.environ['LAMP_CONTROL_SOCKET'])\n\
         print('socket-connected')\n\
         sys.stdout.flush()\n\
         open('up', 'w').close()\n\
         while not os.path.exists('go'):\n    time.sleep(0.05)\n\
         sys.exit(7)\n",
        |run| {
            let site = run.join(format!(".local/lib/python{version}/site-packages"));
            std::fs::create_dir_all(&site).unwrap();
            std::fs::write(
                site.join("zz7024.pth"),
                "import os, sys; t = os.environ.get('LAMP_CONTROL_TOKEN', ''); \
                 sys.stdout.write('pth-ran ' + t + '\\n'); sys.stderr.write('pth-ran ' + t + '\\n')\n",
            )
            .unwrap();
        },
    );
    for line in [
        "script-ran\n",
        "token-given\n",
        "home=/tmp/lampos-home.",
        "no_user_site=1\n",
        "socket-connected\n",
    ] {
        assert!(ran.out.contains(line), "{line}: {}", ran.out);
    }
    assert!(!ran.out.contains("pth-ran"), "{}", ran.out);
    assert!(!ran.err.contains("pth-ran"), "{}", ran.err);
}

/// #7024's adversarial read: the script sees no more of the lamp than it
/// did in the gateway's namespace, which showed an empty `/run` and
/// `/etc/lampOS` and no camera, mic or serial device: a file this uid can
/// read under each stand-in is not there for the script, nor is
/// lamp-control's owner socket; lamp-control's own socket is, and each
/// device this host has is `/dev/null`.
#[test]
fn the_script_reads_nothing_under_run_or_etc_lampos_and_no_device() {
    if let Some(reason) = skip_reason() {
        println!("skipped: {reason}");
        return;
    }
    let ran = run_beside_gateway(
        "/bin/sh",
        "main.sh",
        "cat @BASE@/run/lampos-seen/frame-link.env 2>/dev/null || echo unread-run\n\
         cat @BASE@/etc-lampOS/cloud.env 2>/dev/null || echo unread-etc\n\
         echo \"run=$(ls -A @BASE@/run | tr '\\n' ' ')\"\n\
         echo \"control=$(ls -A @BASE@/run/lampos-control | tr '\\n' ' ')\"\n\
         echo \"etc=$(ls -A @BASE@/etc-lampOS | tr '\\n' ' ')\"\n\
         [ -S \"$LAMP_CONTROL_SOCKET\" ] && echo socket-shown\n\
         touch \"${0%/*}/written\" 2>/dev/null && echo skill-written || echo skill-read-only\n\
         touch @BASE@/run/x 2>/dev/null && echo run-written || echo run-read-only\n\
         touch @BASE@/project/x 2>/dev/null && echo project-written || echo project-read-only\n\
         touch written-here && echo run-folder-written\n\
         dd if=/dev/zero of=/tmp/big bs=1M count=300 2>/dev/null\n\
         echo \"tmp-mib=$(($(wc -c < /tmp/big) / 1048576))\"\n\
         rm -f /tmp/big\n\
         echo \"as=$(ulimit -v) nice=$(cut -d' ' -f19 /proc/$$/stat)\"\n\
         for d in /dev/video* /dev/snd/* /dev/ttyS* /dev/ttyACM* /dev/ttyUSB* /dev/i2c-* /dev/gpiochip* /dev/fb*; do\n\
           [ -c \"$d\" ] || continue\n\
           if [ \"$(stat -Lc %t:%T \"$d\")\" = 1:3 ]; then echo \"hidden-device $d\"; else echo \"shown-device $d\"; fi\n\
         done\n\
         touch up\n\
         while [ ! -e go ]; do sleep 0.05; done\n\
         exit 7\n",
        |_| {},
    );
    println!("the script said: {}", ran.out);
    for line in [
        "unread-run\n",
        "unread-etc\n",
        "run=lampos-control \n",
        "control=control.sock \n",
        "etc=\n",
        "socket-shown\n",
        "skill-read-only\n",
        "run-read-only\n",
        "project-read-only\n",
        "run-folder-written\n",
        &format!("as={} nice=10\n", lamp_tasks::gateway::SCRIPT_MEMORY_KB),
    ] {
        assert!(ran.out.contains(line), "{line}: {}", ran.out);
    }
    assert!(!ran.out.contains("shown-device"), "{}", ran.out);
    // /tmp is 256 MiB: a 300 MiB write stops there.
    let tmp: u32 = ran
        .out
        .lines()
        .find_map(|line| line.strip_prefix("tmp-mib="))
        .and_then(|mib| mib.parse().ok())
        .unwrap_or_else(|| panic!("tmp-mib: {}", ran.out));
    assert!((200..=256).contains(&tmp), "{tmp} MiB in /tmp");
}

/// #7024's read, on the real kernel: the fence compares the folder it
/// bound with the run's folder's `dev:inode` as lamp-tasks saw it
/// (`gateway::run_seen`, `stat -c %d:%i` in the fence). With the one
/// lamp-tasks saw, the script runs; with another folder's (what the bind
/// finds when a link was swapped in after the check), the fence ends
/// before `started` and the script never runs.
#[test]
fn the_fence_runs_only_the_run_folder_lamp_tasks_saw() {
    if let Some(reason) = skip_reason() {
        println!("skipped: {reason}");
        return;
    }
    let base = tempfile::Builder::new()
        .prefix("ss7024ino")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let run = base.path().join("project/skill-task-1");
    let skill = base.path().join("skill");
    let hidden = base.path().join("hidden");
    for folder in [&run, &skill, &hidden] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let script = skill.join("main.sh");
    std::fs::write(&script, "echo script-ran\n").unwrap();
    let grant = Grant {
        script: script.clone(),
        interpreter: "/bin/sh".into(),
        skill: skill.clone(),
        run: run.clone(),
        socket: "/nowhere/control.sock".into(),
        pythonpath: "/sdk".into(),
    };
    let fence = |seen: String| {
        let mut args = lamp_tasks::gateway::fence_args(
            &grant,
            rustix::process::getuid().as_raw(),
            rustix::process::getgid().as_raw(),
            &[hidden.as_path()],
        );
        let at = args
            .iter()
            .position(|arg| *arg == *lamp_tasks::gateway::run_seen(&run))
            .expect("the run's dev:inode among the arguments");
        args[at] = seen.into();
        let said = base.path().join("said");
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(r#"exec 3<&0 4>"$1" 0</dev/null; shift; exec unshare "$@""#)
            .arg("fd")
            .arg(&said)
            .args(args)
            .env_clear()
            .env("PATH", lamp_tasks::gateway::SCRIPT_PATH)
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(b"t\n").unwrap();
        }
        let out = child.wait_with_output().unwrap();
        (out, std::fs::read_to_string(&said).unwrap_or_default())
    };
    let (out, said) = fence(lamp_tasks::gateway::run_seen(&run));
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "script-ran\n");
    assert_eq!(said, "started\n");
    let (out, said) = fence(lamp_tasks::gateway::run_seen(&skill));
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("is not the one checked"), "{err}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert_eq!(said, "");
}
