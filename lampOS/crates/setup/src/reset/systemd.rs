//! A target stop job can finish before its PartOf services stop. Reset waits
//! for every named member job, then verifies the same graph is inactive.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const TARGET: &str = "lampos.target";
const MAX_UNITS: usize = 256;
const MAX_OUTPUT: usize = 64 * 1024;
const STOP_LIMIT: Duration = Duration::from_secs(60);

pub(super) fn stop() -> io::Result<()> {
    stop_with(&mut command, Instant::now() + STOP_LIMIT)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn time_left(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "reset stop deadline",
        ))
    } else {
        Ok(())
    }
}

fn unit_name(name: &str) -> bool {
    let Some((prefix, suffix)) = name.rsplit_once('.') else {
        return false;
    };
    if prefix.is_empty()
        || name.len() > 255
        || ![
            "service",
            "target",
            "socket",
            "timer",
            "path",
            "scope",
            "slice",
            "mount",
            "automount",
            "swap",
            "device",
        ]
        .contains(&suffix)
    {
        return false;
    }
    let bytes = name.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\\' {
            if at + 3 >= bytes.len()
                || bytes[at + 1] != b'x'
                || !bytes[at + 2].is_ascii_hexdigit()
                || !bytes[at + 3].is_ascii_hexdigit()
            {
                return false;
            }
            at += 4;
        } else if bytes[at].is_ascii_alphanumeric() || b":-_.@".contains(&bytes[at]) {
            at += 1;
        } else {
            return false;
        }
    }
    true
}

fn properties(bytes: &[u8], expected: &[&str]) -> io::Result<BTreeMap<String, String>> {
    if bytes.len() > MAX_OUTPUT {
        return Err(invalid("systemctl output exceeds reset bound"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("systemctl output is not UTF-8"))?;
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("malformed systemctl property"))?;
        if !expected.contains(&name) || fields.insert(name.into(), value.into()).is_some() {
            return Err(invalid("unknown or repeated systemctl property"));
        }
    }
    if fields.len() != expected.len()
        || fields.get("LoadState").map(String::as_str) != Some("loaded")
    {
        return Err(invalid(
            "reset unit is not loaded or properties are missing",
        ));
    }
    Ok(fields)
}

fn show(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    unit: &str,
    field: &str,
    until: Instant,
) -> io::Result<BTreeMap<String, String>> {
    time_left(until)?;
    let output = run(
        &[
            "show".into(),
            "--all".into(),
            "--property=LoadState".into(),
            format!("--property={field}"),
            "--".into(),
            unit.into(),
        ],
        until,
    )?;
    time_left(until)?;
    properties(&output, &["LoadState", field])
}

fn members(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    until: Instant,
) -> io::Result<BTreeSet<String>> {
    let mut found = BTreeSet::from([TARGET.to_owned()]);
    let mut pending = vec![TARGET.to_owned()];
    while let Some(unit) = pending.pop() {
        let fields = show(run, &unit, "ConsistsOf", until)?;
        let value = &fields["ConsistsOf"];
        if unit == TARGET && value.trim().is_empty() {
            return Err(invalid("reset target has no discovered members"));
        }
        for member in value.split_ascii_whitespace() {
            if !unit_name(member) {
                return Err(invalid("invalid reset member unit name"));
            }
            if found.insert(member.into()) {
                if found.len() > MAX_UNITS {
                    return Err(invalid("reset member count exceeds bound"));
                }
                pending.push(member.into());
            }
        }
    }
    Ok(found)
}

fn stop_with(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    until: Instant,
) -> io::Result<()> {
    let before = members(run, until)?;
    let mut args = vec!["stop".into(), "--".into()];
    args.extend(before.iter().cloned());
    time_left(until)?;
    let output = run(&args, until)?;
    time_left(until)?;
    if !output.is_empty() {
        return Err(invalid("unexpected systemctl stop output"));
    }
    if members(run, until)? != before {
        return Err(invalid("reset membership changed while stopping"));
    }
    for unit in before {
        let fields = show(run, &unit, "ActiveState", until)?;
        if fields["ActiveState"] != "inactive" {
            return Err(invalid("reset member is not inactive after stop"));
        }
    }
    time_left(until)
}

struct OwnedChild {
    child: Child,
    reaped: bool,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Only the CLI's owned group is killed. Its systemd jobs may continue;
        // a failed barrier leaves the reset journal for recovery after reboot.
        if self.reaped {
            return;
        }
        if let Some(group) = i32::try_from(self.child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn command(args: &[String], until: Instant) -> io::Result<Vec<u8>> {
    time_left(until)?;
    command_with(
        Command::new("/usr/bin/systemctl")
            .arg("--no-ask-password")
            .args(args)
            .env("SYSTEMD_PAGER", "cat"),
        until,
    )
}

fn command_with(command: &mut Command, until: Instant) -> io::Result<Vec<u8>> {
    time_left(until)?;
    let mut owned = OwnedChild {
        child: command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?,
        reaped: false,
    };
    let mut output = owned
        .child
        .stdout
        .take()
        .ok_or_else(|| invalid("missing systemctl output"))?;
    let flags = rustix::fs::fcntl_getfl(&output)?;
    rustix::fs::fcntl_setfl(&output, flags | rustix::fs::OFlags::NONBLOCK)?;
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let mut eof = false;
    let mut status = None;
    loop {
        time_left(until)?;
        loop {
            match output.read(&mut buffer) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(count) => {
                    if bytes.len() + count > MAX_OUTPUT {
                        return Err(invalid("systemctl output exceeds reset bound"));
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            time_left(until)?;
        }
        // Keep the leader unreaped while a descendant could still hold its
        // output pipe. This retains ownership of the process-group identity
        // until either EOF or timeout cleanup, without risking PID reuse.
        if eof && status.is_none() {
            status = owned.child.try_wait()?;
            owned.reaped = status.is_some();
        }
        if eof && let Some(status) = status {
            return if status.success() {
                Ok(bytes)
            } else {
                Err(io::Error::other("systemctl reset stop operation failed"))
            };
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Manager {
        graph: BTreeMap<String, String>,
        after: Option<BTreeMap<String, String>>,
        stopped: bool,
        fail: Option<&'static str>,
        active: &'static str,
        calls: Vec<Vec<String>>,
        late_writer: bool,
        progress: Option<std::path::PathBuf>,
    }
    impl Manager {
        fn new() -> Self {
            Self {
                graph: BTreeMap::from([
                    (
                        TARGET.into(),
                        "lamp-voice.service lamp-memory.service".into(),
                    ),
                    ("lamp-voice.service".into(), String::new()),
                    ("lamp-memory.service".into(), String::new()),
                ]),
                after: None,
                stopped: false,
                fail: None,
                active: "inactive",
                calls: Vec::new(),
                late_writer: true,
                progress: None,
            }
        }
        fn run(&mut self, args: &[String], _: Instant) -> io::Result<Vec<u8>> {
            self.calls.push(args.to_vec());
            if self.fail == Some(args[0].as_str()) {
                return Err(io::Error::other("injected command failure"));
            }
            if args[0] == "stop" {
                // Target completion alone leaves the writer pending. Explicit
                // member jobs flush it before the caller can start wiping.
                if args.contains(&"lamp-voice.service".into()) {
                    self.flush_writer()?;
                }
                self.stopped = true;
                if let Some(after) = self.after.take() {
                    self.graph = after;
                }
                return Ok(Vec::new());
            }
            let unit = args.last().unwrap();
            let field = args
                .iter()
                .find(|arg| arg.starts_with("--property=") && *arg != "--property=LoadState")
                .unwrap();
            if field == "--property=ConsistsOf" {
                let value = self
                    .graph
                    .get(unit)
                    .ok_or_else(|| invalid("unknown fake unit"))?;
                Ok(format!("LoadState=loaded\nConsistsOf={value}\n").into_bytes())
            } else {
                assert!(self.stopped);
                let state = self.active;
                Ok(format!("LoadState=loaded\nActiveState={state}\n").into_bytes())
            }
        }
        fn flush_writer(&mut self) -> io::Result<()> {
            if self.late_writer {
                if let Some(path) = &self.progress {
                    std::fs::write(path, b"synthetic-old-outcomes-at-16:50")?;
                }
                self.late_writer = false;
            }
            Ok(())
        }
        fn stop(&mut self) -> io::Result<()> {
            stop_with(
                &mut |args, until| self.run(args, until),
                Instant::now() + STOP_LIMIT,
            )
        }
    }

    #[test]
    fn recursive_members_are_deduplicated_and_cycles_terminate() {
        let mut manager = Manager::new();
        manager.graph.insert(
            TARGET.into(),
            "nested.target nested.target lamp-voice.service".into(),
        );
        manager.graph.insert(
            "nested.target".into(),
            "lampos.target lamp-memory.service".into(),
        );
        manager.stop().unwrap();
        let stop = manager.calls.iter().find(|args| args[0] == "stop").unwrap();
        assert_eq!(
            stop,
            &[
                "stop",
                "--",
                "lamp-memory.service",
                "lamp-voice.service",
                "lampos.target",
                "nested.target"
            ]
        );
        assert!(!stop.iter().any(|arg| arg == "--wait"));
    }

    #[test]
    fn late_shutdown_write_is_wiped_before_reset_reboots() {
        use std::{
            fs,
            os::unix::fs::DirBuilderExt,
            sync::atomic::{AtomicU64, Ordering},
        };
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Root(std::path::PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "reset-writer-barrier-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let root = Root(path);
        let private = root.0.join("voice/onboarding");
        fs::create_dir_all(&private).unwrap();
        fs::create_dir_all(root.0.join("setup")).unwrap();
        let progress = private.join("progress.json");
        let mut old = Manager::new();
        old.progress = Some(progress.clone());
        old.run(&["stop".into(), "--".into(), TARGET.into()], Instant::now())
            .unwrap();
        super::super::wipe(&private).unwrap();
        old.flush_writer().unwrap();
        assert_eq!(
            fs::read(&progress).unwrap(),
            b"synthetic-old-outcomes-at-16:50",
            "anchor-only completion permits a later shutdown flush to resurrect old data"
        );

        struct Effects {
            manager: Manager,
            progress: std::path::PathBuf,
            entered: std::sync::mpsc::SyncSender<()>,
            release: std::sync::mpsc::Receiver<()>,
        }
        impl super::super::Effects for Effects {
            fn run(&mut self, argv: &[String]) -> io::Result<()> {
                if argv[1] == "stop" {
                    // Model the anchor job finishing while a member's shutdown
                    // flush is still held. Only the explicit member job waits.
                    self.manager
                        .run(&["stop".into(), "--".into(), TARGET.into()], Instant::now())?;
                    stop_with(
                        &mut |args, until| {
                            if args[0] == "stop" {
                                self.entered.send(()).map_err(io::Error::other)?;
                                self.release
                                    .recv_timeout(Duration::from_secs(5))
                                    .map_err(io::Error::other)?;
                            }
                            self.manager.run(args, until)
                        },
                        Instant::now() + STOP_LIMIT,
                    )
                } else {
                    assert!(!self.progress.exists(), "wipe must finish before reboot");
                    Ok(())
                }
            }
            fn sync(&mut self) {}
            fn force_reboot(&mut self) {
                panic!("unexpected reboot failure");
            }
        }
        let marker = root.0.join("setup/done");
        fs::write(&marker, b"done").unwrap();
        let plan = super::super::Plan {
            marker,
            journal: root.0.join("setup/reset.json"),
            wipe: vec![private.clone()],
            stop: vec!["/usr/bin/systemctl".into(), "stop".into(), TARGET.into()],
            reboot: vec!["/usr/bin/systemctl".into(), "reboot".into()],
            detach: vec![],
        };
        let mut manager = Manager::new();
        manager.progress = Some(progress.clone());
        let (entered, waiting) = std::sync::mpsc::sync_channel(1);
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let mut effects = Effects {
            manager,
            progress: progress.clone(),
            entered,
            release: released,
        };
        let worker_plan = plan.clone();
        let worker = std::thread::spawn(move || {
            let result = super::super::reset(&worker_plan, &mut effects);
            (result, effects)
        });
        let reached = waiting.recv_timeout(Duration::from_secs(5));
        let before_release = (
            progress.exists(),
            plan.marker.exists(),
            plan.journal.exists(),
        );
        // Release and join even if a pre-release observation was wrong, so a
        // failed assertion cannot strand the reset fixture's worker.
        let _ = release.send(());
        let (result, mut effects) = worker.join().unwrap();
        assert!(reached.is_ok(), "reset never reached the held member stop");
        assert_eq!(
            before_release,
            (true, false, true),
            "progress stays until member completion; marker is gone and journal remains"
        );
        result.unwrap();
        effects.manager.flush_writer().unwrap();
        assert!(
            !progress.exists(),
            "no writer remains to restore old outcomes"
        );
        assert_eq!(fs::read_dir(private).unwrap().count(), 0);
        assert!(!plan.journal.exists() && !plan.marker.exists());
    }

    #[test]
    fn subprocess_output_and_original_deadline_are_bounded() {
        let error = command_with(
            Command::new("/bin/sh").args(["-c", "exec /bin/sleep 10"]),
            Instant::now() + Duration::from_millis(30),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let error = command_with(
            Command::new("/bin/sh").args(["-c", "while :; do printf '%04096d' 0; done"]),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        use std::{
            fs,
            os::unix::fs::DirBuilderExt,
            sync::atomic::{AtomicU64, Ordering},
        };
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Root(std::path::PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let path = std::env::temp_dir().join(format!(
            "reset-descendant-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let root = Root(path);
        let ready = root.0.join("ready");
        let escaped = root.0.join("escaped");
        let error = command_with(
            Command::new("/bin/sh")
                .arg("-c")
                .arg("(printf ready > \"$1\"; /bin/sleep 3; printf escaped > \"$2\") & exit 0")
                .arg("held-output")
                .arg(&ready)
                .arg(&escaped),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        // The leader exited, but its descendant held stdout. Group cleanup
        // must kill that descendant before its delayed write, not merely reap
        // the leader and leave a process behind. These test-only margins also
        // let the descendant start while other build and I/O gates are busy.
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            ready.is_file(),
            "descendant did not reach the pipe-holding control"
        );
        assert!(
            !escaped.exists(),
            "descendant escaped timeout group cleanup"
        );
    }

    #[test]
    fn changed_membership_or_active_member_refuses_the_wipe() {
        let mut manager = Manager::new();
        manager.after = Some(BTreeMap::from([
            (TARGET.into(), "lamp-voice.service".into()),
            ("lamp-voice.service".into(), String::new()),
        ]));
        assert!(manager.stop().is_err());
        for state in [
            "active",
            "deactivating",
            "failed",
            "activating",
            "reloading",
            "",
        ] {
            let mut manager = Manager::new();
            manager.active = state;
            assert!(manager.stop().is_err(), "{state}");
        }
    }

    #[test]
    fn malformed_missing_unknown_and_oversized_properties_fail_closed() {
        for bytes in [
            b"ConsistsOf=lamp-voice.service\n".to_vec(),
            b"LoadState=not-found\nConsistsOf=\n".to_vec(),
            b"LoadState=loaded\nConsistsOf=\nConsistsOf=x.service\n".to_vec(),
            b"LoadState=loaded\nConsistsOf=\nOther=x\n".to_vec(),
            vec![b'x'; MAX_OUTPUT + 1],
            vec![0xff],
        ] {
            assert!(properties(&bytes, &["LoadState", "ConsistsOf"]).is_err());
        }
        for value in [
            "other*.service",
            "--all",
            "../x.service",
            "bad\\q.service",
            "valid.service\0",
        ] {
            let mut manager = Manager::new();
            manager.graph.insert(TARGET.into(), value.into());
            assert!(manager.stop().is_err());
        }
        let mut manager = Manager::new();
        manager.graph.insert(TARGET.into(), String::new());
        assert!(manager.stop().is_err());
    }

    #[test]
    fn membership_count_and_original_deadline_are_bounded() {
        let mut manager = Manager::new();
        manager.graph.insert(
            TARGET.into(),
            (0..MAX_UNITS)
                .map(|n| format!("unit-{n}.service"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        assert!(manager.stop().is_err());
        assert!(
            stop_with(
                &mut |_, _| panic!("expired deadline cannot run a command"),
                Instant::now()
            )
            .is_err()
        );
    }

    #[test]
    fn query_and_stop_failures_are_not_successful_stops() {
        for fail in ["show", "stop"] {
            let mut manager = Manager::new();
            manager.fail = Some(fail);
            assert!(manager.stop().is_err());
        }
    }

    #[test]
    fn existing_stock_systemctl_and_keeper_plans_use_the_barrier() {
        for program in ["/usr/bin/systemctl", "/bin/systemctl"] {
            assert!(super::super::systemd_stop(&[
                program.into(),
                "stop".into(),
                TARGET.into()
            ]));
        }
        assert!(super::super::keeper_stop(&[
            "/usr/local/bin/lamp-keeper".into(),
            "stop".into(),
            "lamp".into()
        ]));
        assert!(!super::super::systemd_stop(&[
            "/usr/bin/systemctl".into(),
            "stop".into(),
            "other.target".into()
        ]));
        assert!(!super::super::keeper_stop(&[
            "/usr/local/bin/lamp-keeper".into(),
            "restart".into(),
            "lamp".into()
        ]));
    }
}
