use lamp_ipc::monotonic_us;
use lamp_turn_owner::{Controller, MonoTime};
use lamp_voice::{
    process::{SessionDirectory, Worker, new_boot},
    wire::{Control, WorkerEvent},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[test]
fn real_worker_handshake_control_and_finite_shutdown() {
    // No other test's socket is copied into these children (`spawns`).
    let _spawning = crate::spawns::spawning();
    let dir = SessionDirectory::create().unwrap();
    let boot = new_boot().unwrap();
    let mut worker = Worker::spawn(
        Path::new(env!("CARGO_BIN_EXE_lamp-voice")),
        &dir.path,
        "probe",
        boot,
        &[],
    )
    .unwrap();
    let mut owner = Controller::new(boot, MonoTime::from_micros(monotonic_us()));
    worker
        .channels
        .control
        .send(Control::Authority {
            snapshot: owner
                .snapshot(MonoTime::from_micros(monotonic_us()))
                .unwrap(),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        if let Some(WorkerEvent::Ready) = worker.channels.control.receive().unwrap() {
            break;
        }
        assert!(Instant::now() < deadline, "control receipt timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
    worker.shutdown().unwrap();
}

/// #1071: a worker role the voice's list does not have is never started,
/// neither by the voice nor by its executable, so a new role that was not
/// put on the list (the one the records guard reads) fails here first.
#[test]
fn a_worker_role_not_on_the_voices_list_is_never_started() {
    // No other test's socket is copied into these children (`spawns`).
    let _spawning = crate::spawns::spawning();
    let executable = Path::new(env!("CARGO_BIN_EXE_lamp-voice"));
    let dir = SessionDirectory::create().unwrap();
    let error = Worker::spawn(executable, &dir.path, "third", new_boot().unwrap(), &[])
        .err()
        .expect("an unlisted role is refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("`third`"), "{error}");
    let output = std::process::Command::new(executable)
        .args(["worker", "third", "/nonexistent", "parent", "worker"])
        .env_clear()
        .output()
        .unwrap();
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("not on the voice's list"), "{said}");
    // Every role on the list gets past that refusal to its own arguments.
    for role in lamp_voice_processes::WorkerRole::ALL {
        let output = std::process::Command::new(executable)
            .args(["worker", role.as_str(), "/nonexistent", "parent", "worker"])
            .env_clear()
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(
            !said.contains("not on the voice's list"),
            "{role:?}: {said}"
        );
    }
}

/// #1071 (Twelve's P3 on #1095): every first word the voice's `main.rs`
/// dispatches is on its list, and a word not on it is refused before
/// anything runs, so a command added there and not to the list fails here.
#[test]
fn every_command_the_voice_dispatches_is_on_its_list_and_no_other_runs() {
    // No other test's socket is copied into these children (`spawns`).
    let _spawning = crate::spawns::spawning();
    let source = include_str!("../../src/main.rs");
    let mut dispatched = std::collections::BTreeSet::new();
    for marker in ["command==\"", "command == \""] {
        for (at, _) in source.match_indices(marker) {
            let rest = &source[at + marker.len()..];
            let word = &rest[..rest.find('"').expect("a closed command word")];
            dispatched.insert(word);
        }
    }
    assert!(dispatched.contains("serve") && dispatched.contains("worker"));
    for word in &dispatched {
        assert!(
            lamp_voice_processes::Command::parse(word).is_some(),
            "`{word}` is dispatched by main.rs but not on lamp-voice-processes' list"
        );
    }
    let listed: std::collections::BTreeSet<_> = lamp_voice_processes::Command::ALL
        .iter()
        .map(|command| command.as_str())
        .collect();
    assert_eq!(
        listed, dispatched,
        "the list names a command main.rs has not"
    );
    let executable = Path::new(env!("CARGO_BIN_EXE_lamp-voice"));
    let output = std::process::Command::new(executable)
        .args(["third", "/nonexistent"])
        .env_clear()
        .output()
        .unwrap();
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("command `third` is not on the voice's list"),
        "{said}"
    );
}

/// The voice's helpers on its list are the programs it starts beside itself.
#[test]
fn the_voices_listed_helpers_are_the_ones_it_starts() {
    // No other test's socket is copied into these children (`spawns`).
    let _spawning = crate::spawns::spawning();
    assert_eq!(
        lamp_voice_processes::HELPERS,
        [lamp_turn_end_wire::WORKER_EXECUTABLE]
    );
}

/// #1608, with `LAMP_WORKER_SCHEDULING=on`: a worker the voice starts takes
/// its role's scheduling on its own first thread, so it and every thread it
/// makes run that way. The probe is an ordinary role: started from a thread
/// at real-time RR 20 where the host allows it, else at `SCHED_BATCH` (which
/// any thread may take), it and all its threads run at `SCHED_OTHER`. Off,
/// every thread keeps what it started with, as before.
#[cfg(target_os = "linux")]
#[test]
fn a_worker_the_voice_starts_takes_its_roles_scheduling_on_every_thread() {
    // No other test's socket is copied into these children (`spawns`).
    let _spawning = crate::spawns::spawning();
    use thread_priority::{
        NormalThreadSchedulePolicy, RealtimeThreadSchedulePolicy, ThreadPriority,
        ThreadPriorityValue, ThreadSchedulePolicy, set_thread_priority_and_policy,
        thread_native_id,
    };
    assert_eq!(
        lamp_voice_processes::WorkerRole::Probe.scheduling(),
        lamp_voice_processes::Scheduling::Ordinary
    );
    // A thread's policy, from /proc: field 41 of its `stat`, counted after
    // the name's closing parenthesis. SCHED_OTHER 0, SCHED_RR 2, SCHED_BATCH 3.
    fn policy(stat: &std::path::Path) -> u32 {
        let text = std::fs::read_to_string(stat).unwrap();
        let after = &text[text.rfind(')').unwrap() + 2..];
        after.split(' ').nth(38).unwrap().parse().unwrap()
    }
    // The probe's threads' policies, and the policy it was started from.
    fn started(on: bool) -> (u32, Vec<u32>) {
        std::thread::spawn(move || {
            let thread = thread_native_id();
            let realtime = set_thread_priority_and_policy(
                thread,
                ThreadPriority::Crossplatform(ThreadPriorityValue::try_from(20u8).unwrap()),
                ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::RoundRobin),
            )
            .is_ok();
            if !realtime {
                set_thread_priority_and_policy(
                    thread,
                    ThreadPriority::Min,
                    ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Batch),
                )
                .unwrap();
            }
            let dir = SessionDirectory::create().unwrap();
            let mut worker = Worker::spawn_given(
                Path::new(env!("CARGO_BIN_EXE_lamp-voice")),
                &dir.path,
                "probe",
                (new_boot().unwrap(), new_boot().unwrap()),
                &[],
                || Ok(()),
                |name| (on && name == "LAMP_WORKER_SCHEDULING").then(|| "on".into()),
            )
            .unwrap();
            let tasks = format!("/proc/{}/task", worker.process_id());
            let policies: Vec<u32> = std::fs::read_dir(tasks)
                .unwrap()
                .map(|task| policy(&task.unwrap().path().join("stat")))
                .collect();
            worker.shutdown().unwrap();
            (if realtime { 2 } else { 3 }, policies)
        })
        .join()
        .unwrap()
    }
    let (from, on) = started(true);
    assert!(!on.is_empty());
    assert!(
        on.iter().all(|policy| *policy == 0),
        "on, from {from}: {on:?}"
    );
    let (from, off) = started(false);
    assert!(
        off.iter().all(|policy| *policy == from),
        "off, from {from}: {off:?}"
    );
}
