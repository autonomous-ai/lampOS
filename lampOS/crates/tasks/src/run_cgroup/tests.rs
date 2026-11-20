use super::*;
use crate::skill_task::{MEASURED_PEAK_MEMORY_MB, MEASURED_TASKS, MEMORY_HIGH_MB, TASKS_MAX};

/// The unified line of `/proc/self/cgroup` is read; a v1 system's lines
/// name none, and `serve` already in its leaf is under the unit's root.
#[test]
fn its_own_cgroup_is_read_from_the_unified_line() {
    let unit = "/lampos.slice/lampos-parts.slice/lamp-tasks.service";
    assert_eq!(own_cgroup(&format!("0::{unit}\n")), Some(unit));
    assert_eq!(
        own_cgroup(&format!("12:memory:/x\n1:name=systemd:/x\n0::{unit}\n")),
        Some(unit)
    );
    assert_eq!(own_cgroup("12:memory:/x\n1:name=systemd:/x\n"), None);
    assert_eq!(own_cgroup("0::\n"), None);
    assert_eq!(root_of(&format!("{unit}/serve")), unit);
    assert_eq!(root_of(unit), unit);
    assert_eq!(root_of("/serve"), "/serve");
}

/// The limits, exactly as written, and why they fit: a run at its
/// `memory.max` beside a Hermes task's measured peak stays under the
/// unit's `MemoryHigh`, and its tasks under `TasksMax`.
#[test]
fn a_runs_limits_are_its_whole_total_within_the_units() {
    let written: Vec<(&str, String)> = limits().into_iter().collect();
    assert_eq!(
        written,
        vec![
            ("memory.max", "512M".to_owned()),
            ("memory.high", "448M".to_owned()),
            ("memory.swap.max", "0".to_owned()),
            ("cpu.max", "100000 100000".to_owned()),
            ("pids.max", "72".to_owned()),
        ]
    );
    const { assert!(RUN_MEMORY_HIGH_MB < RUN_MEMORY_MAX_MB) };
    const { assert!(MEASURED_PEAK_MEMORY_MB + RUN_MEMORY_MAX_MB < MEMORY_HIGH_MB) };
    const { assert!(MEASURED_TASKS + RUN_PIDS_MAX < TASKS_MAX) };
    // Never wider than the fence's own process cap allows the script.
    const { assert!(RUN_PIDS_MAX > crate::skill_run::SCRIPT_PROCESSES) };
    assert_eq!(CONTROLLERS, "+memory +cpu +pids");
    let delegated: Vec<String> = DELEGATE["Delegate=".len()..]
        .split(' ')
        .map(|name| format!("+{name}"))
        .collect();
    assert_eq!(delegated.join(" "), CONTROLLERS);
}

/// Fail closed (Heron, #8180): with no delegated cgroup a run is refused
/// on Linux, never run without its total; off Linux it runs with the
/// fence's caps alone.
#[test]
fn without_a_delegated_cgroup_a_run_is_refused_on_linux() {
    let missing = RunCgroups::Missing("no Delegate=".into());
    assert_eq!(missing.for_run_on("task-1", true).unwrap_err(), NO_CGROUP);
    assert!(missing.for_run_on("task-1", false).unwrap().is_none());
}

/// A limit that can't be written refuses the run (on every system: a
/// delegated cgroup that takes no limit is no bound), and its cgroup is
/// not kept.
#[test]
fn a_limit_not_written_refuses_the_run() {
    let root = tempfile::tempdir().unwrap();
    let cgroups = RunCgroups::Fake(root.path().to_owned());
    // `cpu.max` as a folder: it can't be written.
    std::fs::create_dir_all(root.path().join("run-bad/cpu.max")).unwrap();
    assert_eq!(cgroups.for_run_on("bad", false).unwrap_err(), NO_CGROUP);
    assert_eq!(
        RunCgroups::Fake(root.path().join("none"))
            .for_run_on("x", false)
            .unwrap_err(),
        NO_CGROUP
    );
}

/// `memory.swap.max` is written `0` whenever the kernel has it, and
/// never made where it has not.
#[test]
fn no_swap_is_written_whenever_the_kernel_has_it() {
    let root = tempfile::tempdir().unwrap();
    let cgroups = RunCgroups::Fake(root.path().to_owned());
    std::fs::create_dir(root.path().join("run-swap")).unwrap();
    std::fs::write(root.path().join("run-swap/memory.swap.max"), "max").unwrap();
    let with = cgroups.for_run_on("swap", true).unwrap().unwrap();
    assert_eq!(
        std::fs::read_to_string(with.dir().join("memory.swap.max")).unwrap(),
        "0"
    );
    for (file, value) in limits() {
        assert_eq!(
            std::fs::read_to_string(with.dir().join(file)).unwrap(),
            value
        );
    }
    let without = cgroups.for_run_on("plain", true).unwrap().unwrap();
    assert!(!without.dir().join("memory.swap.max").exists());
    assert_eq!(
        std::fs::read_to_string(without.dir().join("memory.max")).unwrap(),
        "512M"
    );
}

/// At `serve`'s start every pid of the unit's cgroup is moved into its
/// leaf, then the controllers are handed down; a stale run's empty
/// cgroup is removed.
#[test]
fn serve_moves_into_its_leaf_and_hands_the_controllers_down() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("cgroup.procs"), "4242\n").unwrap();
    std::fs::create_dir(root.path().join("run-stale")).unwrap();
    set_up_at(root.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("serve/cgroup.procs")).unwrap(),
        "4242"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("cgroup.subtree_control")).unwrap(),
        CONTROLLERS
    );
    assert!(!root.path().join("run-stale").exists());
    // Unreadable root: refused, said why.
    assert!(set_up_at(&root.path().join("none")).is_err());
}

/// On a lamp (Linux, a delegated cgroup v2 with `+memory +pids` in its
/// `subtree_control` and no process of its own, named by
/// `LAMP_TASKS_CGROUP_TEST_ROOT`, e.g. under `systemd-run --user -p
/// Delegate=yes`): a process past its run's `memory.max` is killed, the
/// test process (as `serve`) stays, and the run's cgroup is removed.
/// Skipped elsewhere.
#[cfg(target_os = "linux")]
#[test]
fn a_run_past_its_memory_max_is_killed_while_serve_stays() {
    let Some(root) = std::env::var_os("LAMP_TASKS_CGROUP_TEST_ROOT") else {
        eprintln!("skipped: no LAMP_TASKS_CGROUP_TEST_ROOT (a delegated cgroup) here");
        return;
    };
    let root = PathBuf::from(root);
    let cgroup = RunCgroups::Own(root.clone())
        .for_run("memory-test")
        .unwrap()
        .unwrap();
    let mut command = std::process::Command::new("python3");
    command.args([
        "-c",
        &format!("b = bytearray({} * 1024 * 1024)", RUN_MEMORY_MAX_MB + 64),
    ]);
    cgroup.join_at_exec(&mut command);
    let status = command.status().unwrap();
    assert!(!status.success(), "{status:?}");
    let events = std::fs::read_to_string(cgroup.dir().join("memory.events")).unwrap();
    assert!(
        events
            .lines()
            .any(|line| line.starts_with("oom_kill ") && line != "oom_kill 0"),
        "{events}"
    );
    let dir = cgroup.dir().to_owned();
    drop(cgroup);
    assert!(!dir.exists());
}
