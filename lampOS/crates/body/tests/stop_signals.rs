//! #6834: a customer lamp's body, restarted by systemd, printed no exit
//! report. lamp-body reads SIGINT and SIGTERM from a signalfd, blocked in
//! `main`; the unplug adapter's worker (`LAMP_UNPLUG=on`, as the product
//! unit has it) was made before that, without them blocked, so the kernel
//! gave the process's SIGTERM to the worker, and its default ended the
//! process: no easing back, no report. This program does what lamp-body
//! does, in its order, and checks `main` still takes SIGTERM.
use nix::{
    sys::signal::{SigSet, Signal, kill},
    unistd::Pid,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
};

const CHILD: &str = "LAMP_BODY_STOP_SIGNALS_CHILD";

fn main() {
    if std::env::var_os(CHILD).is_some() {
        let unplug = lamp_body::unplug::Adapter::held(|_| false).unwrap();
        assert!(unplug.is_some(), "the unplug adapter did not start");
        let mut mask = SigSet::empty();
        mask.add(Signal::SIGINT);
        mask.add(Signal::SIGTERM);
        mask.thread_block().unwrap();
        // Stopped from outside, as systemd does.
        println!("ready");
        std::io::stdout().flush().unwrap();
        // lamp-body is busy with a step when it comes, and reads its
        // signalfd after: the signal waits for it, pending.
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert_eq!(mask.wait().unwrap(), Signal::SIGTERM);
        println!("took SIGTERM in main");
        return;
    }
    let root = std::env::temp_dir().join(format!("lamp-body-stop-signals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env(CHILD, "1")
        .env(lamp_body::unplug::ENVIRONMENT, "on")
        .env(lamp_bus::ROOT_VARIABLE, &root)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut said = String::new();
    out.read_line(&mut said).unwrap();
    assert_eq!(said, "ready\n");
    kill(Pid::from_raw(child.id() as i32), Signal::SIGTERM).unwrap();
    out.read_to_string(&mut said).unwrap();
    let status = child.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        status.success() && said.contains("took SIGTERM in main"),
        "SIGTERM blocked in main after the unplug adapter started ended the process: {status:?} {said}"
    );
    println!("stop_signals: ok");
}
