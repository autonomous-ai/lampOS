//! #1445: the voice runs at real-time round-robin priority 20, and the
//! same-voice child it starts must not keep that class. The real
//! `lamp-same-voice serve` is started from a thread at the voice's class,
//! when this machine lets a test take it, and read from outside while it
//! waits to open its model, a FIFO nobody writes: by then it has taken the
//! ordinary policy at `lamp_same_voice::NICE` or nicer. Where a test may
//! not take a real-time class, the parent is ordinary, and the test shows
//! only that the child lowers itself before its model is read.
#![cfg(target_os = "linux")]
#![allow(unsafe_code)]

use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// A FIFO in a folder of its own, standing in for the model.
fn model_never_written() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lamp-same-voice-priority-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let fifo = dir.join("model.onnx");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    fifo
}

#[test]
fn the_spawned_child_does_not_keep_the_voices_real_time_class() {
    let model = model_never_written();
    let (real_time, mut child) = std::thread::spawn(move || {
        let voice = libc::sched_param { sched_priority: 20 };
        // SAFETY: the calling thread's own policy, with a valid parameter
        // that outlives the call.
        let real_time = unsafe { libc::sched_setscheduler(0, libc::SCHED_RR, &voice) } == 0;
        let child = Command::new(env!("CARGO_BIN_EXE_lamp-same-voice"))
            .arg("serve")
            .arg(&model)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        (real_time, child)
    })
    .join()
    .unwrap();
    eprintln!("the parent took the voice's real-time class: {real_time}");
    let pid = child.id() as libc::pid_t;
    let began = Instant::now();
    let (policy, nice) = loop {
        // SAFETY: reads another process's policy and niceness by its id;
        // no memory is passed.
        let (policy, nice) = unsafe {
            (
                libc::sched_getscheduler(pid),
                libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t),
            )
        };
        if policy == libc::SCHED_OTHER && nice >= lamp_same_voice::NICE {
            break (policy, nice);
        }
        if began.elapsed() > Duration::from_secs(10) {
            break (policy, nice);
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        policy,
        libc::SCHED_OTHER,
        "the child kept its parent's class"
    );
    assert!(nice >= lamp_same_voice::NICE, "nice {nice}");
}
