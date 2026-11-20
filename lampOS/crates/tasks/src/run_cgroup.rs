//! Each run's own cgroup (#8180): a skill's run step
//! (`crate::skill_run::DirectRunner`) and a make's tried run
//! (`crate::make_try::FenceLaunch`) each run in a sub-cgroup of their own,
//! `run-<id>`, under lamp-tasks.service's delegated cgroup
//! (`Delegate=memory cpu pids` on the unit), with a real total for the
//! whole run: [`RUN_MEMORY_MAX_MB`] of memory ([`RUN_MEMORY_HIGH_MB`] its
//! throttle point), no swap, [`RUN_CPU_MAX`] (one core) and
//! [`RUN_PIDS_MAX`] tasks. The fence's `ulimit`s and `prlimit` stay, a
//! second line of defence.
//!
//! Without it the only total was `prlimit --nproc=64` times each process's
//! `ulimit -v` of 768 MiB, far past the unit's own `MemoryHigh=2100M`: one
//! run could hold the whole lamp-tasks cgroup in reclaim, `serve` and
//! Harness's `wait` and `watch`es with it.
//!
//! - **At `serve`'s start** ([`set_up`]): its own cgroup read from
//!   `/proc/self/cgroup` (v2, `0::/path`), a leaf [`SERVE_LEAF`] made in
//!   it, every pid of the unit moved there (the no-internal-process rule),
//!   then [`CONTROLLERS`] written to its `cgroup.subtree_control`.
//! - **Per run** ([`RunCgroups::for_run`]): `run-<id>` made, each of
//!   [`limits`] written, the run's first child joining it before its exec
//!   by writing `0` to its `cgroup.procs` ([`RunCgroup::join_at_exec`]);
//!   every process of the run is killed (`cgroup.kill`) and the folder
//!   removed when the run ends, however it ended ([`RunCgroup`]'s drop).
//! - **Fail closed on Linux**: no delegated cgroup (no `Delegate=`, cgroup
//!   v1), or a run's cgroup or any of its limits not made: the run (and a
//!   make's try) is refused with [`NO_CGROUP`], never run without a total.
//!   Off Linux (a developer's Mac, its tests) there is no cgroup, and a run
//!   goes on with the fence's per-process caps alone, said once at start.
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The most memory one run's processes hold together (`memory.max`), in
/// MiB: past it the kernel's OOM killer ends the run's processes, never
/// another of lamp-tasks'. Sized from lamp-tasks.service's measures
/// (lamp-9adf, 12 Nov, `crate::skill_task::MEMORY_HIGH_MB`): 632 MiB at
/// rest, 1120 MiB at a Hermes task's peak; 1120 + 512 = 1632 MiB, under
/// the unit's `MemoryHigh=2100M` with 468 MiB to spare, so one run can't
/// push the unit into reclaim even beside a Hermes task. A run step's
/// Python script with the SDK is about 30 MiB.
pub const RUN_MEMORY_MAX_MB: u64 = 512;
/// Where a run's memory is throttled first (`memory.high`), in MiB: 64
/// MiB under [`RUN_MEMORY_MAX_MB`], so a growing run slows (reclaim of its
/// own pages) before it is killed.
pub const RUN_MEMORY_HIGH_MB: u64 = 448;
/// A run's CPU (`cpu.max`): 100 ms in every 100 ms, one core of the A733's
/// eight, as a gateway agent's turn has (`CPUQuota=100%`); the unit's
/// `CPUQuota=360%` keeps 2.6 cores for `serve` and an agent's task.
pub const RUN_CPU_MAX: &str = "100000 100000";
/// The most tasks (processes and threads) a run's cgroup holds
/// (`pids.max`): the script's [`crate::skill_run::SCRIPT_PROCESSES`] (the
/// fence's `prlimit`) and 8 for the fence's own around it (its outer
/// `unshare`, its `sh`, a net run's relay and its threads). Well under the
/// unit's `TasksMax=512` (126 at a Hermes task's peak).
pub const RUN_PIDS_MAX: u64 = crate::skill_run::SCRIPT_PROCESSES + 8;
/// The leaf `serve` and every other process of the unit are moved into,
/// so the unit's own cgroup holds none and can hand its controllers down.
pub const SERVE_LEAF: &str = "serve";
/// What is written to the unit's `cgroup.subtree_control`: the three the
/// unit delegates (`Delegate=memory cpu pids`).
pub const CONTROLLERS: &str = "+memory +cpu +pids";
/// lamp-tasks.service's line that hands [`CONTROLLERS`] to it.
pub const DELEGATE: &str = "Delegate=memory cpu pids";
/// A run's own cgroup's folder name, before its id.
pub const RUN_PREFIX: &str = "run-";
/// Where cgroup v2 is mounted.
pub const MOUNT: &str = "/sys/fs/cgroup";
/// What a run refused for want of its own cgroup is told (journaled by
/// lamp-skills, and its fixed line said for a run started by words).
pub const NO_CGROUP: &str =
    "the run's own memory and CPU bounds could not be set, so it was not run";

/// Each of a run's limits, written in this order: the file and its value.
/// `memory.swap.max` is written whenever the kernel has it (the lamp has
/// no swap, so a kernel without swap accounting has none to bound).
pub fn limits() -> [(&'static str, String); 5] {
    [
        ("memory.max", format!("{RUN_MEMORY_MAX_MB}M")),
        ("memory.high", format!("{RUN_MEMORY_HIGH_MB}M")),
        ("memory.swap.max", "0".to_owned()),
        ("cpu.max", RUN_CPU_MAX.to_owned()),
        ("pids.max", RUN_PIDS_MAX.to_string()),
    ]
}

/// The limits that are written only when their file is there.
const IF_THERE: [&str; 1] = ["memory.swap.max"];

/// The unified (v2) cgroup path in `/proc/self/cgroup`'s text: its `0::`
/// line's. None for a cgroup v1 or hybrid-only system, or none at all.
pub fn own_cgroup(proc_self_cgroup: &str) -> Option<&str> {
    proc_self_cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .filter(|path| path.starts_with('/'))
}

/// The delegated root for a process in `path`: its own cgroup, or its
/// parent when it is already in [`SERVE_LEAF`] (`serve` set up before).
pub fn root_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((parent, SERVE_LEAF)) if !parent.is_empty() => parent,
        _ => path,
    }
}

/// Where each run's cgroup comes from.
#[derive(Clone, Debug)]
pub enum RunCgroups {
    /// What [`set_up`] found at `serve`'s start, read at each run.
    Lamp,
    /// Runs' cgroups made under this delegated root.
    Own(PathBuf),
    /// None, and why: on Linux every run is refused, elsewhere it runs
    /// with the fence's caps alone.
    Missing(String),
    /// A test's: runs' cgroups made under this plain folder, each kept as
    /// `done-<id>` at its end (a plain folder's files can't be rmdir'd).
    #[cfg(test)]
    Fake(PathBuf),
}

static SET_UP: OnceLock<RunCgroups> = OnceLock::new();

/// Made once at `lamp-tasks serve`'s start (see the module), said in one
/// journal line either way. Called again, it keeps the first.
pub fn set_up() {
    let found = if cfg!(target_os = "linux") {
        match std::fs::read_to_string("/proc/self/cgroup") {
            Err(error) => RunCgroups::Missing(format!("/proc/self/cgroup unread: {error}")),
            Ok(text) => match own_cgroup(&text) {
                None => RunCgroups::Missing("no cgroup v2 (0::) line".into()),
                Some(path) => {
                    let root = Path::new(MOUNT).join(root_of(path).trim_start_matches('/'));
                    match set_up_at(&root) {
                        Ok(()) => RunCgroups::Own(root),
                        Err(why) => RunCgroups::Missing(format!(
                            "{why} (lamp-tasks.service needs Delegate=memory cpu pids)"
                        )),
                    }
                }
            },
        }
    } else {
        RunCgroups::Missing("not Linux".into())
    };
    match &found {
        RunCgroups::Own(root) => eprintln!(
            "[lamp-tasks] each run gets its own cgroup under {}: memory.max {RUN_MEMORY_MAX_MB}M, cpu.max {RUN_CPU_MAX}, pids.max {RUN_PIDS_MAX}",
            root.display()
        ),
        RunCgroups::Missing(why) if cfg!(target_os = "linux") => eprintln!(
            "[lamp-tasks] runs have no own cgroup: {why}; every run step and make's try is refused"
        ),
        RunCgroups::Missing(why) => eprintln!(
            "[lamp-tasks] runs have no own cgroup: {why}; they run with the fence's per-process caps alone"
        ),
        _ => {}
    }
    let _ = SET_UP.set(found);
}

/// [`set_up`] at the delegated `root`: [`SERVE_LEAF`] made, every pid in
/// `root` moved into it, then [`CONTROLLERS`] handed down. Stale runs'
/// cgroups (a `serve` that ended mid-run) are removed.
pub fn set_up_at(root: &Path) -> Result<(), String> {
    let leaf = root.join(SERVE_LEAF);
    match std::fs::create_dir(&leaf) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("{} not made: {error}", leaf.display())),
    }
    // Moved until none is left: a process may fork while they move.
    for _ in 0..10 {
        let procs = std::fs::read_to_string(root.join("cgroup.procs"))
            .map_err(|error| format!("its cgroup.procs unread: {error}"))?;
        let pids: Vec<&str> = procs.split_whitespace().collect();
        if pids.is_empty() {
            break;
        }
        for pid in pids {
            // One that ended meanwhile is no matter.
            let _ = std::fs::write(leaf.join("cgroup.procs"), pid);
        }
    }
    std::fs::write(root.join("cgroup.subtree_control"), CONTROLLERS)
        .map_err(|error| format!("{CONTROLLERS} not handed down: {error}"))?;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(RUN_PREFIX) {
                remove(&entry.path());
            }
        }
    }
    Ok(())
}

impl RunCgroups {
    /// What runs get now: [`RunCgroups::Lamp`] read from [`set_up`]'s.
    fn now(&self) -> RunCgroups {
        match self {
            RunCgroups::Lamp => SET_UP.get().cloned().unwrap_or_else(|| {
                RunCgroups::Missing("lamp-tasks serve did not set them up".into())
            }),
            other => other.clone(),
        }
    }

    /// The run `id`'s own cgroup, its limits written; None off Linux with
    /// none; refused ([`NO_CGROUP`], why journaled) otherwise.
    pub fn for_run(&self, id: &str) -> Result<Option<RunCgroup>, String> {
        self.for_run_on(id, cfg!(target_os = "linux"))
    }

    /// [`RunCgroups::for_run`] as on Linux (`linux`) or not.
    pub fn for_run_on(&self, id: &str, linux: bool) -> Result<Option<RunCgroup>, String> {
        let refused = |why: String| {
            eprintln!("[lamp-tasks] run {id}: refused, no own cgroup: {why}");
            Err(NO_CGROUP.to_owned())
        };
        let (root, fake) = match self.now() {
            RunCgroups::Own(root) => (root, false),
            #[cfg(test)]
            RunCgroups::Fake(root) => (root, true),
            RunCgroups::Missing(why) if linux => return refused(why),
            RunCgroups::Missing(_) | RunCgroups::Lamp => return Ok(None),
        };
        if id.is_empty() || id.contains('/') || id.contains('\0') {
            return refused("its id is no folder name".into());
        }
        let dir = root.join(format!("{RUN_PREFIX}{id}"));
        match make(&dir, fake) {
            Ok(procs) => Ok(Some(RunCgroup { dir, procs, fake })),
            Err(why) => {
                remove(&dir);
                refused(why)
            }
        }
    }
}

/// `dir` made (one already there, as a test places it, is used) and each
/// of [`limits`] written; its `cgroup.procs` path for a child to join.
fn make(dir: &Path, fake: bool) -> Result<CString, String> {
    match std::fs::create_dir(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("{} not made: {error}", dir.display())),
    }
    for (file, value) in limits() {
        let path = dir.join(file);
        if IF_THERE.contains(&file) && !path.exists() {
            continue;
        }
        // Never made: a real cgroup has every file it controls.
        std::fs::OpenOptions::new()
            .write(true)
            .create(fake)
            .truncate(true)
            .open(&path)
            .and_then(|mut opened| std::io::Write::write_all(&mut opened, value.as_bytes()))
            .map_err(|error| format!("{file} {value} not written: {error}"))?;
    }
    let procs = dir.join("cgroup.procs");
    if fake {
        std::fs::write(&procs, "").map_err(|error| format!("cgroup.procs: {error}"))?;
    }
    use std::os::unix::ffi::OsStrExt;
    CString::new(procs.as_os_str().as_bytes()).map_err(|_| "a NUL in its path".to_owned())
}

/// A run's own cgroup, ended with the run: every process in it killed and
/// its folder removed, however the run ended.
#[derive(Debug)]
pub struct RunCgroup {
    dir: PathBuf,
    procs: CString,
    fake: bool,
}

impl RunCgroup {
    /// The cgroup's folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `command`'s child joins this cgroup just before its exec (it writes
    /// `0` to its `cgroup.procs`), so nothing it starts is ever outside it;
    /// a child that can't join fails to start, and the run with it.
    #[allow(unsafe_code)]
    pub fn join_at_exec(&self, command: &mut std::process::Command) {
        use std::os::unix::process::CommandExt;
        let procs = self.procs.clone();
        // SAFETY: the closure runs in the forked child just before its exec
        // and calls only `open`, `write` and `close` (async-signal-safe);
        // the path was made in the parent, nothing is allocated or locked.
        unsafe {
            command.pre_exec(move || {
                let fd = rustix::fs::open(
                    procs.as_c_str(),
                    rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                )?;
                let written = rustix::io::write(&fd, b"0")?;
                if written != 1 {
                    return Err(std::io::Error::other("cgroup.procs: short write"));
                }
                Ok(())
            });
        }
    }
}

impl Drop for RunCgroup {
    fn drop(&mut self) {
        if self.fake {
            let done = self.dir.with_file_name(
                self.dir
                    .file_name()
                    .map(|name| name.to_string_lossy().replacen(RUN_PREFIX, "done-", 1))
                    .unwrap_or_default(),
            );
            let _ = std::fs::rename(&self.dir, done);
            return;
        }
        // Anything of the run left (its PID namespace's last processes
        // still dying) killed at once; Linux 5.14 and later.
        let kill = self.dir.join("cgroup.kill");
        if kill.exists() {
            let _ = std::fs::write(kill, "1");
        }
        remove(&self.dir);
    }
}

/// `dir` removed (rmdir), tried again for up to a second while its last
/// processes are still being reaped (EBUSY); journaled when it stays.
fn remove(dir: &Path) {
    for _ in 0..40 {
        match std::fs::remove_dir(dir) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) if error.raw_os_error() == Some(rustix::io::Errno::BUSY.raw_os_error()) => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(error) => {
                eprintln!("[lamp-tasks] {} not removed: {error}", dir.display());
                return;
            }
        }
    }
    eprintln!("[lamp-tasks] {} not removed: still busy", dir.display());
}

#[cfg(test)]
mod tests;
