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
    assert_eq!(
        lamp_voice_processes::HELPERS,
        [lamp_turn_end_wire::WORKER_EXECUTABLE]
    );
}
