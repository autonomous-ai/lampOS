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
//! - **Its bounds**: [`RUN_WITHIN`] from its start (its whole process group
//!   killed then), at most [`MOST_OUTPUT`] bytes of output (none of it
//!   kept or journaled: past it, the run is ended), one at a time across
//!   the lamp (beside, never behind, an agent's task). lamp-skills never
//!   hands a failed one over again for that trigger.
//! - **Success**: the script exited 0 within its bound, and lamp-control
//!   says at least one of its run's `say`s was said whole (the admin
//!   `end`'s `said`, `crate::skill_task::Tokens::end_said`). Anything else
//!   is `run_failed` with the reason, for lamp-skills to journal (and, for a
//!   run the person's words started, to say its one fixed line).
use crate::protocol::refusal;
use crate::skill_task::{HandOver, PYTHONPATH_ENV, RUN_TICK, Runner, SOCKET_ENV, SkillRun, Tokens};
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
/// What a run step granted `net` is told: its fence has only `lo`.
pub const NO_NET_HERE: &str =
    "a run step's script never reaches the internet: a skill granted net runs its script as a task";
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

/// The gateway's fence ([`crate::gateway::fence_text`], no `net`) with
/// [`CONTROL_EDITS`].
pub fn fence_text() -> String {
    let mut text = crate::gateway::fence_text(false);
    for (from, to) in CONTROL_EDITS {
        text = text.replacen(from, to, 1);
    }
    text
}

/// What the fence's own `sh` is started through: the token's pipe (stdin)
/// moved to fd 3 and the started pipe (stdout) to fd 4, as the launcher
/// places them, stdin then `/dev/null` and stdout the output pipe, before
/// `unshare` is exec'd with the rest of its arguments.
pub const PLACE_FDS: &str = "exec 3<&0 4>&1 0</dev/null 1>&2; exec \"$0\" \"$@\"";

/// Runs a run step's script itself, in its own fence: see the module.
#[derive(Clone, Debug)]
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
        }
    }

    /// The fence's arguments for `run` in its folder `folder`: the
    /// gateway's ([`crate::gateway::fence_args`]) with [`fence_text`] in
    /// place of its text. Never the token.
    pub fn args(&self, run: &SkillRun, folder: &Path) -> Vec<std::ffi::OsString> {
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
            egress: None,
        };
        let hidden: Vec<&Path> = self.hidden.iter().map(PathBuf::as_path).collect();
        let mut args = crate::gateway::fence_args(
            &grant,
            rustix::process::getuid().as_raw(),
            rustix::process::getgid().as_raw(),
            &hidden,
        );
        if let Some(at) = args.iter().position(|arg| arg == "-c") {
            args[at + 1] = fence_text()
                .replace("\"$SCRIPT_PATH\"", crate::gateway::SCRIPT_PATH)
                .into();
        }
        args
    }
}

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
        let token = run
            .env
            .iter()
            .find(|(name, _)| *name == crate::skill_task::TOKEN_ENV)
            .map(|(_, token)| token.clone())
            .unwrap_or_default();
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(PLACE_FDS)
            .arg(&self.unshare)
            .args(self.args(run, &folder))
            .env_clear()
            .env("PATH", crate::gateway::SCRIPT_PATH)
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|error| format!("its fence could not be started: {error}"))?;
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
            Some(Some(0)) if ran => Ok(0),
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
/// token minted as a task's, held to exactly the skill's grants, the script
/// run through `runner` within [`RUN_WITHIN`], the token ended and its
/// said count read ([`Tokens::end_said`]): `{"ok":true,"id":0,"said":N}`
/// for a run that ended well and said something, else the refusal.
pub fn answer(
    hand: &HandOver,
    tokens: &dyn Tokens,
    runner: &dyn Runner,
    keys_named: bool,
) -> Value {
    if hand
        .grants
        .iter()
        .any(|grant| grant == crate::skill_task::NET_GRANT)
    {
        return refusal("bad_request", NO_NET_HERE);
    }
    if keys_named {
        return refusal("bad_request", NO_KEYS_HERE);
    }
    let handed = Instant::now();
    let run_id = crate::skill_task::run_id();
    let minted = match tokens.mint(&hand.skill, &run_id) {
        Ok(minted) => minted,
        Err(why) => return refusal("no_token", &why),
    };
    let asked: std::collections::BTreeSet<&String> = hand.grants.iter().collect();
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
    drop(minted);
    eprintln!(
        "[lamp-tasks] skill run {run_id}: {} handed over, token minted in {} ms",
        hand.skill,
        handed.elapsed().as_millis()
    );
    let ran = runner.run(&run, &|| {});
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
