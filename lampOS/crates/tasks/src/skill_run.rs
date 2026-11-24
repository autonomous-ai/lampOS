//! A skill's `run` step (#8158): its script run by lamp-tasks itself, with
//! no agent and no Harness, in the fence a gateway agent's skill run gets
//! (`crate::gateway::FENCE_TEXT`, #7133), so a made skill speaks within
//! seconds rather than after an agent starts (43 to 53 s on lamp-c0a4).
//!
//! - **The hand-over**: lamp-skills' line is a task's
//!   ([`crate::skill_task::HandOver`]) with `op` `skill_run`. It is checked
//!   whole as a task's is, and its token minted from the owner's yes for the
//!   folder as it is, exactly as a task's ([`answer`]): lamp-control names
//!   the skill's own grants and nothing else.
//! - **Its fence** ([`DirectRunner`]): `unshare` with
//!   [`crate::gateway::fence_args`] and [`fence_text`]: a user (root mapped),
//!   network (only `lo`), mount and PID namespace of its own; a fresh
//!   `/tmp`; every path of [`HIDDEN`] covered by an empty tmpfs, so nothing
//!   of `/var` (every part's state, lamp-tasks' own among it), `/run` (every
//!   part's socket, lamp-control's admin socket among them), `/home` or
//!   `/opt` is there; only the skill's own folder, the run's own folder and
//!   lamp-control's `control.sock` alone ([`CONTROL_EDITS`], `crate::skill_task::CONTROL_SOCKET`) are shown
//!   again. The script then runs in a user namespace of its own (no
//!   capability over these mounts), as the caller's uid, with an
//!   environment of `PATH`, `HOME` (its run's folder), the socket, the SDK
//!   and its one-run token only, read from a pipe. Never as lamp-skills,
//!   never with any of its rights.
//! - **`net`**: the way out a net task gets today, with the same check
//!   (`crate::skill_task::Tokens::net_agreed`, [`NET_SWITCH`] on):
//!   lamp-control agrees for the owner's kept yes naming it, or, with
//!   `LAMP_MADE_CODE_ON_AT_ONCE` on (the product's), for a made skill's
//!   folder as it is with no yes (#8180, `lamp_control::task_yes`). Its own
//!   egress socket `skill-<run id>` ([`crate::egress::Grant`], the public
//!   web on ports 80 and 443 only) on fd 5 of its fence, which
//!   [`crate::gateway::FENCE_NET_EDITS`] relay to (`HTTPS_PROXY`
//!   `127.0.0.1:3128`), lamp-tasks' relay alone shown again under `/opt`
//!   ([`RELAY_EDITS`]). Not agreed: refused before any token. Keys are
//!   never given ([`NO_KEYS_HERE`]).
//! - **Its bounds**: its own cgroup ([`crate::run_cgroup`], #8180: a real
//!   total of memory, CPU and tasks for the whole run, refused on Linux
//!   without it), and its own caps ([`CAPS_EDIT`]: [`SCRIPT_MEMORY_KB`] of
//!   address space, [`SCRIPT_CPU_SECS`] of CPU, niceness [`SCRIPT_NICE`],
//!   no core file, each process; [`SCRIPT_PROCESSES`] for the whole run),
//!   never lamp-tasks' whole budget; [`RUN_WITHIN`] from
//!   its start (its whole process group killed then), at most [`MOST_OUTPUT`] bytes of output (none of it
//!   kept or journaled: past it, the run is ended), one at a time across
//!   the lamp (beside, never behind, an agent's task). lamp-skills never
//!   hands a failed one over again for that trigger.
//! - **Success**: the script exited 0 within its bound, and lamp-control
//!   says at least one of its run's `say`s was said whole (the admin
//!   `end`'s `said`, `crate::skill_task::Tokens::end_said`). Anything else
//!   is `run_failed` with the reason, for lamp-skills to journal (and, for a
//!   run the person's words started, to say its one fixed line).
use crate::protocol::refusal;
use crate::skill_task::{
    HandOver, NET_GRANT, NET_SWITCH, PYTHONPATH_ENV, RUN_FOLDER_PREFIX, RUN_TICK, Runner,
    SOCKET_ENV, SkillRun, Tokens,
};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The `op` of a `run` step's hand-over line.
pub const OP: &str = "skill_run";
/// The longest a run step's script runs, from its fence's start.
pub const RUN_WITHIN: Duration = Duration::from_secs(30);
/// The most bytes a run step's script may write (stdout and stderr
/// together) before it is ended. None of it is kept.
pub const MOST_OUTPUT: usize = 64 * 1024;
/// Where lamp-tasks makes each run's own folder (`run-<run id>`), under its
/// own state folder; hidden from the run with all of `/var`.
pub const PROJECT: &str = "/var/lib/lampos-tasks/runs";
/// What a run step's fence covers with an empty tmpfs (those that are
/// there): lamp-tasks.service's namespace shows all of these, where the
/// gateway's showed none. The skill's folder, the run's folder and
/// `crate::skill_task::CONTROL_SOCKET` alone are shown again.
pub const HIDDEN: [&str; 8] = [
    "/var", "/run", "/home", "/opt", "/srv", "/mnt", "/media", "/dev/shm",
];
/// The program the fence is started with.
pub const UNSHARE: &str = "/usr/bin/unshare";
/// What lamp-tasks says of a run step whose script ended well but said
/// nothing aloud.
pub const SAID_NOTHING: &str = "its script ended well but nothing it asked lamp.say for was said";
/// What a run step granted `net` is told when lamp-control doesn't agree
/// to net for its folder as it is: a net task's words.
pub const NO_NET_YES: &str = "the owner's yes for this skill's folder as it is doesn't name net, so its run doesn't reach the internet";
/// What a run step whose skill names keys is told: none is given.
pub const NO_KEYS_HERE: &str =
    "a run step's script is given no keys: a skill that names keys runs its script as a task";
/// What a run step is told when no direct runner is set up.
pub const OFF: &str = "a skill's run steps are not run on this lamp";

/// What [`fence_text`] changes in the gateway's fence, each found exactly
/// once (a test holds it): lamp-control's `control.sock` bound to a staging
/// file in the fresh `/tmp` before anything is hidden, and moved back,
/// alone, into an otherwise empty `/run/lampos-control` once the hidden
/// paths are covered (`$so` is under `/run`). Nothing else of the fence
/// changes.
pub const CONTROL_EDITS: [(&str, &str); 2] = [
    (
        "mount -n --bind \"$k\" \"$t2\" || no \"the skill's folder\"\n",
        concat!(
            "mount -n --bind \"$k\" \"$t2\" || no \"the skill's folder\"\n",
            "t3=$(mktemp /tmp/lampos-sock.XXXXXX) && mount -n --bind \"$so\" \"$t3\" || no \"lamp-control's socket\"\n",
        ),
    ),
    (
        "mount -n --move \"$t2\" \"$k\" || no \"the skill's folder back\"\n",
        concat!(
            "mkdir -p \"${so%/*}\" && touch \"$so\" && mount -n --move \"$t3\" \"$so\" && rm -f \"$t3\" || no \"lamp-control's socket back\"\n",
            "mount -n --move \"$t2\" \"$k\" || no \"the skill's folder back\"\n",
        ),
    ),
];

/// What [`fence_text`] changes too for a run granted `net`, the same way
/// as [`CONTROL_EDITS`]: lamp-tasks' own program
/// ([`crate::gateway::RELAY`], which [`crate::gateway::FENCE_NET_EDITS`]
/// start the run's relay with) bound to a staging file before `/opt` is
/// hidden, and moved back alone once it is. Nothing else of `/opt` is
/// shown.
pub const RELAY_EDITS: [(&str, &str); 2] = [
    (
        "mount -n --bind \"$k\" \"$t2\" || no \"the skill's folder\"\n",
        concat!(
            "mount -n --bind \"$k\" \"$t2\" || no \"the skill's folder\"\n",
            "t4=$(mktemp /tmp/lampos-relay.XXXXXX) && mount -n --bind /opt/lampOS/tasks/current/lamp-tasks \"$t4\" || no \"lamp-tasks' relay\"\n",
        ),
    ),
    (
        "mount -n --move \"$t2\" \"$k\" || no \"the skill's folder back\"\n",
        concat!(
            "mkdir -p /opt/lampOS/tasks/current && touch /opt/lampOS/tasks/current/lamp-tasks && mount -n --move \"$t4\" /opt/lampOS/tasks/current/lamp-tasks && rm -f \"$t4\" || no \"lamp-tasks' relay back\"\n",
            "mount -n --move \"$t2\" \"$k\" || no \"the skill's folder back\"\n",
        ),
    ),
];

/// The script's own address space cap, in KiB (`ulimit -v`, soft and
/// hard): 768 MiB, the per-script cap #7520 gives a gateway agent's
/// script. Past it an allocation fails (Python's `MemoryError`), so the
/// run fails; nothing else of the lamp is touched.
pub const SCRIPT_MEMORY_KB: u64 = 768 * 1024;
/// The script's own CPU time cap, in seconds (`ulimit -t`): [`RUN_WITHIN`]'s,
/// so over its run it has at most one core's worth on average, as a
/// gateway agent's turns have (`CPUQuota=100%` on its unit); past it the
/// kernel ends it.
pub const SCRIPT_CPU_SECS: u64 = 30;
/// The script's niceness (`nice -n`): a gateway agent's unit's `Nice=10`,
/// so a busy script yields the CPU to the voice and lamp-tasks' own work.
pub const SCRIPT_NICE: u32 = 10;

/// The most processes and threads the whole run may hold at once
/// (`RLIMIT_NPROC`, #8180): its script's, every child's and its relay's
/// together. Past it a fork or a new thread fails (Python's
/// `BlockingIOError`), so a script that spawns too many fails its run.
pub const SCRIPT_PROCESSES: u64 = 64;

/// What [`fence_text`] changes last: the script's own caps, set just
/// before its exec and after anything else of the fence (its relay
/// included) started: no core file (`ulimit -c 0`), [`SCRIPT_MEMORY_KB`],
/// [`SCRIPT_CPU_SECS`] and [`SCRIPT_NICE`] for each process, and
/// [`SCRIPT_PROCESSES`] for the run as a whole, so the per-process caps
/// bound the whole: at most 64 processes of 768 MiB and 30 s of CPU each,
/// within the 30 s wall clock, its PID namespace ended then.
///
/// The process cap is `prlimit`'s (util-linux, as `unshare` is), since the
/// fence's `/bin/sh` is dash on a lamp, whose flag for it is `-p` where
/// bash's is `-u`. It is set in the fence's own user namespace, just before
/// the script's nested one is made, so the kernel counts it per namespace
/// (Linux 5.14 and later): the run's own processes, never lamp-tasks' or
/// any other of `lampos`'s. These are a second line of defence: the run's
/// real total is its own cgroup's (`crate::run_cgroup`, #8180:
/// `memory.max`, `cpu.max` and `pids.max` in lamp-tasks.service's
/// delegated cgroup).
pub const CAPS_EDIT: (&str, &str) = (
    "\nexec unshare --user --map-user=\"$u\"",
    concat!(
        "\nulimit -c 0 || no \"no core file\"\n",
        "ulimit -v 786432 || no \"the script's memory cap\"\n",
        "ulimit -t 30 || no \"the script's CPU time cap\"\n",
        "prlimit --nproc=64:64 --pid $$ || no \"the run's process cap\"\n",
        "exec nice -n 10 unshare --user --map-user=\"$u\"",
    ),
);

/// The gateway's fence ([`crate::gateway::fence_text`], with its way out
/// when `net`) with [`CONTROL_EDITS`], [`RELAY_EDITS`] when `net`, and
/// [`CAPS_EDIT`].
pub fn fence_text(net: bool) -> String {
    let mut text = crate::gateway::fence_text(net);
    let relay: &[(&str, &str)] = if net { &RELAY_EDITS } else { &[] };
    for (from, to) in CONTROL_EDITS.iter().chain(relay).chain([&CAPS_EDIT]) {
        text = text.replacen(from, to, 1);
    }
    text
}

/// A run step's fence's `unshare` arguments for `grant`, `hidden` covered:
/// the gateway's ([`crate::gateway::fence_args`]) with [`fence_text`] in
/// place of its text (its way out when `grant` names an egress socket).
/// One builder for a run step and a make's tried run
/// (`crate::make_try::fence_args`, #8180), so the two can't drift. Never
/// the token.
pub fn fence_args(grant: &crate::skill_start::Grant, hidden: &[&Path]) -> Vec<std::ffi::OsString> {
    let mut args = crate::gateway::fence_args(
        grant,
        rustix::process::getuid().as_raw(),
        rustix::process::getgid().as_raw(),
        hidden,
    );
    if let Some(at) = args.iter().position(|arg| arg == "-c") {
        args[at + 1] = fence_text(grant.egress.is_some())
            .replace("\"$SCRIPT_PATH\"", crate::gateway::SCRIPT_PATH)
            .into();
    }
    args
}

/// What the fence's own `sh` is started through: the token's pipe (stdin)
/// moved to fd 3 and the started pipe (stdout) to fd 4, as the launcher
/// places them, stdin then `/dev/null` and stdout the output pipe, before
/// `unshare` is exec'd with the rest of its arguments.
pub const PLACE_FDS: &str = "exec 3<&0 4>&1 0</dev/null 1>&2; exec \"$0\" \"$@\"";

/// Runs a run step's script itself, in its own fence: see the module.
#[derive(Clone)]
pub struct DirectRunner {
    /// `unshare` ([`UNSHARE`]); a test's stand-in elsewhere.
    pub unshare: PathBuf,
    /// Where each run's own folder is made ([`PROJECT`]).
    pub project: PathBuf,
    /// What the fence covers ([`HIDDEN`]).
    pub hidden: Vec<PathBuf>,
    /// What the script is run with (`crate::gateway::INTERPRETER`).
    pub interpreter: String,
    pub tick: Duration,
    /// [`MOST_OUTPUT`].
    pub most_output: usize,
    /// Where a run granted `net` gets its own egress socket
    /// ([`crate::egress::Grant::lamp`], with [`NET_SWITCH`] on); None: such
    /// a run is refused before it starts.
    pub egress: Option<crate::egress::Grant>,
    /// Where each run's own cgroup is made (#8180,
    /// [`crate::run_cgroup`]): its real memory, CPU and process total.
    pub cgroups: crate::run_cgroup::RunCgroups,
}

impl std::fmt::Debug for DirectRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectRunner")
            .field("unshare", &self.unshare)
            .field("project", &self.project)
            .field("egress", &self.egress.as_ref().map(|grant| &grant.dir))
            .finish_non_exhaustive()
    }
}

impl DirectRunner {
    /// The lamp's: [`UNSHARE`], [`PROJECT`], [`HIDDEN`], `python3`.
    pub fn lamp() -> Self {
        Self {
            unshare: PathBuf::from(UNSHARE),
            project: PathBuf::from(PROJECT),
            hidden: HIDDEN.iter().map(PathBuf::from).collect(),
            interpreter: crate::gateway::INTERPRETER.to_owned(),
            tick: RUN_TICK,
            most_output: MOST_OUTPUT,
            egress: None,
            cgroups: crate::run_cgroup::RunCgroups::Lamp,
        }
    }

    /// The fence's arguments for `run` in its folder `folder`, its egress
    /// socket `egress` when granted `net`: the gateway's
    /// ([`crate::gateway::fence_args`]) with [`fence_text`] in place of its
    /// text. Never the token.
    pub fn args(
        &self,
        run: &SkillRun,
        folder: &Path,
        egress: Option<&Path>,
    ) -> Vec<std::ffi::OsString> {
        let env_of = |name: &str| {
            run.env
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        let grant = crate::skill_start::Grant {
            script: run.script.clone(),
            interpreter: self.interpreter.clone(),
            skill: run.folder.clone(),
            run: folder.to_owned(),
            socket: env_of(SOCKET_ENV),
            pythonpath: env_of(PYTHONPATH_ENV),
            token: String::new(),
            egress: egress.map(Path::to_owned),
        };
        let hidden: Vec<&Path> = self.hidden.iter().map(PathBuf::as_path).collect();
        fence_args(&grant, &hidden)
    }

    /// A run granted `net`'s way out (see the module): its own egress
    /// socket, served until `until`, and an `O_PATH` descriptor of it for
    /// its fence's fd 5. Refused in words, nothing kept, with [`NET_SWITCH`]
    /// off or off Linux.
    fn way_out(
        &self,
        run: &SkillRun,
        until: Instant,
    ) -> Result<(crate::egress::Served, std::os::fd::OwnedFd), String> {
        let refused = |why: String| Err(crate::skill_task::net_refused(&why));
        let Some(grant) = &self.egress else {
            return refused(format!("{NET_SWITCH} is off in lamp-tasks"));
        };
        let socket = format!("{RUN_FOLDER_PREFIX}{}", run.run);
        let served = match grant.open(&socket, until) {
            Ok(served) => served,
            Err(error) => {
                return refused(format!("the run's way out was not made: {}", error.kind()));
            }
        };
        let fd = match crate::skill_start::egress_fd(served.path()) {
            Ok(fd) => fd,
            Err(error) => {
                return refused(format!(
                    "its egress socket could not be handed to its fence: {}",
                    error.kind()
                ));
            }
        };
        eprintln!(
            "[lamp-tasks] skill run {}: granted net: its egress socket {socket}.sock made, its fence relays to it alone",
            run.run
        );
        Ok((served, fd))
    }
}

/// `egress` at [`crate::skill_start::EGRESS_FD`] in `command`'s child,
/// kept across its exec, for the fence's relay
/// ([`crate::gateway::FENCE_NET_EDITS`]).
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn egress_at_fd_5(command: &mut std::process::Command, egress: &std::os::fd::OwnedFd) {
    use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    let raw = egress.as_raw_fd();
    let at = crate::skill_start::EGRESS_FD;
    // SAFETY: the closure runs in the forked child just before its exec,
    // where only `fcntl` and `dup2` are called (async-signal-safe, nothing
    // allocated or locked). `raw` is held open by the parent until `spawn`
    // returns, so it is open in the child; fd 5 is only overwritten by
    // `dup2`, never closed here (ManuallyDrop).
    unsafe {
        command.pre_exec(move || {
            let source = BorrowedFd::borrow_raw(raw);
            if raw == at {
                let flags = rustix::io::fcntl_getfd(source)?;
                rustix::io::fcntl_setfd(source, flags.difference(rustix::io::FdFlags::CLOEXEC))?;
                return Ok(());
            }
            let mut placed = std::mem::ManuallyDrop::new(OwnedFd::from_raw_fd(at));
            rustix::io::dup2(source, &mut placed)?;
            Ok(())
        });
    }
}

/// Off Linux no descriptor is made ([`DirectRunner::way_out`] refuses).
#[cfg(not(target_os = "linux"))]
fn egress_at_fd_5(_: &mut std::process::Command, _: &std::os::fd::OwnedFd) {}

/// Its own folder, removed however the run ends.
struct RunFolder(PathBuf);

impl Drop for RunFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Runner for DirectRunner {
    fn run(&self, run: &SkillRun, _end_token: &dyn Fn()) -> Result<u64, String> {
        use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
        use std::os::unix::fs::DirBuilderExt;
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        let began = Instant::now();
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.project)
            .map_err(|error| format!("the runs' folder was not made: {error}"))?;
        let folder = self.project.join(format!("run-{}", run.run));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&folder)
            .map_err(|error| format!("the run's own folder was not made: {error}"))?;
        let _folder = RunFolder(folder.clone());
        // Its way out before anything starts: refused, nothing runs.
        let way_out = if run.net {
            Some(self.way_out(run, began + run.deadline)?)
        } else {
            None
        };
        // Its own cgroup before anything starts: none, it is refused
        // (`crate::run_cgroup::NO_CGROUP`) and nothing runs.
        let cgroup = self.cgroups.for_run(&run.run)?;
        let token = run
            .env
            .iter()
            .find(|(name, _)| *name == crate::skill_task::TOKEN_ENV)
            .map(|(_, token)| token.clone())
            .unwrap_or_default();
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(PLACE_FDS)
            .arg(&self.unshare)
            .args(self.args(
                run,
                &folder,
                way_out.as_ref().map(|(served, _)| served.path()),
            ))
            .env_clear()
            .env("PATH", crate::gateway::SCRIPT_PATH)
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        if let Some(cgroup) = &cgroup {
            cgroup.join_at_exec(&mut command);
        }
        if let Some((_, fd)) = &way_out {
            egress_at_fd_5(&mut command, fd);
        }
        let spawned = command.spawn();
        // The child holds its own; the socket stays served until the run
        // ends (`way_out` dropped then).
        let way_out = way_out.map(|(served, _)| served);
        let mut child =
            spawned.map_err(|error| format!("its fence could not be started: {error}"))?;
        let pid = Pid::from_child(&child);
        // The token through a pipe, never an argument or the environment.
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(format!("{token}\n").as_bytes());
        }
        drop(token);
        // The fence says `started` just before it execs the script.
        let started = Arc::new(AtomicBool::new(false));
        let started_at = Arc::new(std::sync::Mutex::new(None::<Duration>));
        if let Some(stdout) = child.stdout.take() {
            let (started, started_at) = (started.clone(), started_at.clone());
            let _ = std::thread::Builder::new()
                .name("tasks-skill-run-started".into())
                .spawn(move || {
                    let mut line = String::new();
                    if BufReader::new(stdout).read_line(&mut line).is_ok()
                        && line.trim_end() == crate::skill_start::STARTED
                    {
                        *started_at.lock().unwrap_or_else(|p| p.into_inner()) =
                            Some(began.elapsed());
                        started.store(true, Ordering::Release);
                    }
                });
        }
        // Its output counted and thrown away; the fence's own first words
        // kept, said only for a fence that never started the script (no
        // script ran, so no word of a script is ever journaled).
        let over = Arc::new(AtomicBool::new(false));
        let first = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let output = child.stderr.take().and_then(|mut stderr| {
            let (over, first, most) = (over.clone(), first.clone(), self.most_output);
            let started = started.clone();
            std::thread::Builder::new()
                .name("tasks-skill-run-output".into())
                .spawn(move || {
                    let mut buffer = [0u8; 4096];
                    let mut total = 0usize;
                    while let Ok(read) = stderr.read(&mut buffer) {
                        if read == 0 {
                            break;
                        }
                        total += read;
                        if !started.load(Ordering::Acquire) {
                            let mut first = first.lock().unwrap_or_else(|p| p.into_inner());
                            let room = 200usize.saturating_sub(first.len());
                            first.extend_from_slice(&buffer[..read.min(room)]);
                        }
                        if total > most {
                            over.store(true, Ordering::Release);
                        }
                    }
                })
                .ok()
        });
        let status = loop {
            match waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            ) {
                Ok(Some(status)) => break Some(status.exit_status()),
                Ok(None) => {}
                Err(error) if error == rustix::io::Errno::INTR => {}
                Err(_) => break None,
            }
            if began.elapsed() >= run.deadline || over.load(Ordering::Acquire) || run.cancelled() {
                break None;
            }
            std::thread::sleep(self.tick);
        };
        let _ = kill_process_group(pid, Signal::KILL);
        let _ = child.kill();
        let _ = child.wait();
        // What is left of the run killed and its cgroup removed.
        drop(cgroup);
        drop(way_out);
        if let Some(output) = output {
            let _ = output.join();
        }
        let ran = started.load(Ordering::Acquire);
        run.started.store(ran, Ordering::Release);
        let at = *started_at.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(at) = at {
            eprintln!(
                "[lamp-tasks] skill run {}: its script started {} ms into its run",
                run.run,
                at.as_millis()
            );
        }
        let over = over.load(Ordering::Acquire);
        match status {
            _ if over => Err(format!(
                "its script wrote more than {} KiB, so it was ended",
                self.most_output / 1024
            )),
            Some(Some(0)) if ran => Ok(at.map_or(0, |at| at.as_millis() as u64)),
            _ if !ran => {
                let first = first.lock().unwrap_or_else(|p| p.into_inner()).clone();
                let said = String::from_utf8_lossy(&first)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect::<String>();
                Err(format!(
                    "its fence was not made, so its script didn't run: {said}"
                ))
            }
            Some(Some(code)) => Err(format!("its script ended with status {code}")),
            Some(None) => Err("its script was ended by a signal".into()),
            None if run.cancelled() => Err("the run was cancelled".into()),
            None if began.elapsed() >= run.deadline => Err(format!(
                "its script was ended at its {} s limit",
                run.deadline.as_secs()
            )),
            None => Err("its script could not be waited for".into()),
        }
    }
}

/// lamp-tasks' answer to a `run` step's hand-over, `hand` checked: its
/// token minted as a task's, held to exactly the skill's grants (`net`
/// only as lamp-control agrees, as a net task's: the owner's yes naming
/// it, or a made skill's with `LAMP_MADE_CODE_ON_AT_ONCE` on, #8180), the script
/// run through `runner` within [`RUN_WITHIN`], the token ended and its
/// said count read ([`Tokens::end_said`]): `{"ok":true,"id":0,"said":N}`
/// for a run that ended well and said something, else the refusal.
pub fn answer(
    hand: &HandOver,
    tokens: &dyn Tokens,
    runner: &dyn Runner,
    keys_named: bool,
) -> Value {
    if keys_named {
        return refusal("bad_request", NO_KEYS_HERE);
    }
    // `net` gets its way out only with lamp-control agreeing for this
    // folder as it is, as a net task's: the owner's yes naming it, or a
    // made skill's with `LAMP_MADE_CODE_ON_AT_ONCE` on (#8180), or any
    // skill's with `LAMP_TASK_NO_YES` on in lamp-control (#8475), no yes
    // kept. Refused here: no token is minted and no egress socket made.
    let net = hand.grants.iter().any(|grant| grant == NET_GRANT);
    if net && !tokens.net_agreed(&hand.skill, &hand.sha256, &hand.grants) {
        return refusal("no_token", NO_NET_YES);
    }
    let handed = Instant::now();
    let run_id = crate::skill_task::run_id();
    // Asked for the owner's named ask (#8475): lamp-control marks the run.
    let minted = match tokens.mint_for(&hand.skill, &run_id, hand.asked_aloud) {
        Ok(minted) => minted,
        Err(why) => return refusal("no_token", &why),
    };
    // The token names lamp-control's grants, never `net`, which is
    // lamp-tasks' own.
    let asked: std::collections::BTreeSet<&String> = hand
        .grants
        .iter()
        .filter(|grant| *grant != NET_GRANT)
        .collect();
    if minted
        .grants
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        != asked
    {
        tokens.end(&run_id);
        return refusal(
            "no_token",
            "the owner's yes for this skill names other grants than its run asks: \
             the owner says yes again in the app",
        );
    }
    let mut run = SkillRun::of_run(hand, &run_id, &minted.token);
    run.deadline = RUN_WITHIN;
    run.net = net;
    drop(minted);
    eprintln!(
        "[lamp-tasks] skill run {run_id}: {} handed over, token minted in {} ms",
        hand.skill,
        handed.elapsed().as_millis()
    );
    let to_runner = handed.elapsed();
    let ran = runner.run(&run, &|| {});
    // Ok: how long into its run its script started; with the hand-over's
    // own, what a ROW reads (#8158's latency).
    if let Ok(into_run) = &ran {
        eprintln!(
            "[lamp-tasks] skill run {run_id}: hand-over to script start {} ms",
            to_runner.as_millis() as u64 + into_run
        );
    }
    // Its says counted as its token ends: none can be said after.
    let said = tokens.end_said(&run_id);
    let began = run.started.load(Ordering::Acquire);
    crate::skill_task::tell_run_ended(&hand.skill, &run_id, ran.is_ok() && said > Some(0));
    eprintln!(
        "[lamp-tasks] skill run {run_id}: ended in {} ms, {}, said {}",
        handed.elapsed().as_millis(),
        match &ran {
            Ok(_) => "its script exited 0".to_owned(),
            Err(why) => why.clone(),
        },
        said.map_or_else(|| "unknown".to_owned(), |said| said.to_string())
    );
    match (ran, said) {
        (Ok(_), Some(said)) if said > 0 => json!({"ok": true, "id": 0, "said": said}),
        (Ok(_), Some(_)) => refusal("run_failed", SAID_NOTHING),
        (Ok(_), None) => refusal(
            "run_failed",
            "its script ended well but lamp-control couldn't say whether anything was said",
        ),
        (Err(why), _) if !began => refusal("not_started", &why),
        (Err(why), _) => refusal("run_failed", &why),
    }
}

#[cfg(test)]
mod tests;
