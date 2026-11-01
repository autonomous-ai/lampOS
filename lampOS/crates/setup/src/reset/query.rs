//! Bounded read-only reset queries. Stop commands keep their own allowances.
//! Setup's hotspot and station reads (`serve::phones`, `serve::station_ip`)
//! use the same bound.
use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

// The updater's setup-unit query uses JOURNAL_WITHIN (five seconds) too.
pub(crate) const QUERY_WITHIN: Duration = Duration::from_secs(5);
const MAX_OUTPUT: usize = 64 * 1024;

pub(super) fn read(argv: &[String]) -> io::Result<String> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty reset query"))?;
    let bytes = output_within(
        Command::new(program).args(args),
        Instant::now() + QUERY_WITHIN,
    )?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

struct OwnedChild {
    child: Child,
    reaped: bool,
}

impl OwnedChild {
    fn stop_group(&mut self) {
        if self.reaped {
            return;
        }
        // An unreaped leader retains the process-group identity. Never signal
        // that identity after reaping: its PID could then belong to another job.
        let group = rustix::process::Pid::from_child(&self.child);
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        let _ = self.child.kill();
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        self.stop_group();
        let _ = self.child.wait();
    }
}

fn time_left(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "reset query timed out",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn output_within(command: &mut Command, until: Instant) -> io::Result<Vec<u8>> {
    time_left(until)?;
    let mut owned = OwnedChild {
        child: command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // System::read previously captured but never exposed stderr.
            // Discard it directly instead of retaining an unbounded buffer.
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?,
        reaped: false,
    };
    let mut output = owned.child.stdout.take().expect("piped query stdout");
    let flags = rustix::fs::fcntl_getfl(&output)?;
    rustix::fs::fcntl_setfl(&output, flags | rustix::fs::OFlags::NONBLOCK)?;
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let eof = loop {
            time_left(until)?;
            match output.read(&mut buffer) {
                Ok(0) => break true,
                Ok(count) => {
                    if bytes.len() + count > MAX_OUTPUT {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "reset query output exceeds 64 KiB",
                        ));
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break false,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        };
        // A descendant may hold the pipe after the leader exits. Retain the
        // unreaped leader until EOF so timeout cleanup still owns its group.
        if eof {
            // Observe exit without consuming the leader. A failed leader may
            // leave descendants that closed stdout; its group must be stopped
            // before reaping releases the identity, just as on timeout.
            let exited = match rustix::process::waitid(
                rustix::process::WaitId::Pid(rustix::process::Pid::from_child(&owned.child)),
                rustix::process::WaitIdOptions::EXITED
                    | rustix::process::WaitIdOptions::NOHANG
                    | rustix::process::WaitIdOptions::NOWAIT,
            ) {
                Ok(exited) => exited,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error @ rustix::io::Errno::CHILD) => {
                    // An external reaper or inherited SIGCHLD disposition
                    // removed our proof of ownership. Never signal that PID.
                    owned.reaped = true;
                    return Err(error.into());
                }
                Err(error) => return Err(error.into()),
            };
            if let Some(exited) = exited {
                if exited.exit_status() != Some(0) {
                    owned.stop_group();
                }
                let status = match owned.child.wait() {
                    Ok(status) => {
                        owned.reaped = true;
                        status
                    }
                    Err(error) => {
                        if error.raw_os_error() == Some(rustix::io::Errno::CHILD.raw_os_error()) {
                            owned.reaped = true;
                        }
                        return Err(error);
                    }
                };
                return if status.success() {
                    Ok(bytes)
                } else {
                    Err(io::Error::other(format!("reset query ended with {status}")))
                };
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }
    fn query(script: &str) -> io::Result<Vec<u8>> {
        output_within(&mut shell(script), Instant::now() + Duration::from_secs(3))
    }

    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "reset-query-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn query_reads_output_with_closed_stdin_and_discarded_stderr() {
        assert_eq!(
            query("if read line; then exit 1; fi; printf active; printf hidden >&2").unwrap(),
            b"active"
        );
        assert_eq!(
            read(&["/bin/sh".into(), "-c".into(), "printf inactive".into()]).unwrap(),
            "inactive"
        );
    }

    #[test]
    fn query_refuses_nonzero_exit_and_spawn_failure() {
        assert_eq!(
            query("printf partial; exit 7").unwrap_err().kind(),
            io::ErrorKind::Other
        );
        let root = Root::new();
        assert_eq!(
            output_within(
                &mut Command::new(root.0.join("missing")),
                Instant::now() + Duration::from_secs(3)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn a_failed_query_cleans_its_same_group_child_even_after_output_closes() {
        let root = Root::new();
        let ready = root.0.join("closed-output-ready");
        let escaped = root.0.join("escaped");
        let release = root.0.join("release");
        let error = output_within(
            shell(
                "(exec 1>&- 2>&-; printf ready > \"$1\"; \
                 attempts=0; \
                 while [ ! -f \"$3\" ] && [ \"$attempts\" -lt 250 ]; do \
                   /bin/sleep 0.02; attempts=$((attempts+1)); \
                 done; \
                 [ ! -f \"$3\" ] || printf escaped > \"$2\") & \
                 attempts=0; \
                 while [ ! -f \"$1\" ] && [ \"$attempts\" -lt 100 ]; do \
                   /bin/sleep 0.02; attempts=$((attempts+1)); \
                 done; \
                 [ -f \"$1\" ] || exit 99; exit 7",
            )
            .arg("closed-output-descendant")
            .arg(&ready)
            .arg(&escaped)
            .arg(&release),
            Instant::now() + Duration::from_secs(4),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(
            error.to_string().contains("exit status: 7"),
            "expected the witnessed leader failure, not setup failure: {error}"
        );
        assert!(ready.is_file(), "child must close output before exit 7");
        // Release the child only after observing the leader's failure, so a
        // delayed leader cannot let it write before cleanup is required.
        fs::write(&release, b"go").unwrap();
        // The fixture exits naturally after this write even with the old bug.
        // Never signal its group after the query may have reaped the leader.
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            !escaped.exists(),
            "failed query left its closed-output descendant running"
        );
    }

    #[test]
    fn query_accepts_64_kib_and_refuses_more() {
        let full = "i=0; while [ $i -lt 16 ]; do printf '%04096d' 0; i=$((i+1)); done";
        assert_eq!(query(full).unwrap().len(), MAX_OUTPUT);
        assert_eq!(
            query(&format!("{full}; printf x")).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn system_read_enforces_the_query_output_bound() {
        use super::super::{Effects, System};
        let full = "i=0; while [ $i -lt 16 ]; do printf '%04096d' 0; i=$((i+1)); done";
        let mut system = System;
        let argv = |script: String| vec!["/bin/sh".into(), "-c".into(), script];
        assert_eq!(system.read(&argv(full.into())).unwrap().len(), MAX_OUTPUT);
        assert_eq!(
            system
                .read(&argv(format!("{full}; printf x")))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn query_deadline_also_applies_after_stdout_closes() {
        let root = Root::new();
        let ready = root.0.join("stdout-closed");
        let error = output_within(
            shell("exec 1>&-; printf ready > \"$1\"; exec /bin/sleep 30")
                .arg("closed-output")
                .arg(&ready),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            ready.is_file(),
            "deadline control must reach stdout closure"
        );
    }

    #[test]
    fn query_timeout_cleans_the_exited_leaders_pipe_holding_descendant() {
        let root = Root::new();
        let ready = root.0.join("ready");
        let escaped = root.0.join("escaped");
        let error = output_within(
            shell("(printf ready > \"$1\"; /bin/sleep 3; printf escaped > \"$2\") & exit 0")
                .arg("pipe-holder")
                .arg(&ready)
                .arg(&escaped),
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            ready.is_file(),
            "descendant must reach the pipe-holding control"
        );
        assert!(!escaped.exists(), "descendant survived owned-group cleanup");
    }

    #[test]
    fn real_query_error_keeps_reset_data_and_journal_and_reboots() {
        use super::super::{Effects, Plan, Step, reset};
        struct EffectsWithQuery {
            rebooted: bool,
            queried: bool,
        }
        impl Effects for EffectsWithQuery {
            fn run(&mut self, argv: &[String]) -> io::Result<()> {
                assert_eq!(argv, &["/bin/systemctl", "reboot"]);
                self.rebooted = true;
                Ok(())
            }
            fn read(&mut self, _: &[String]) -> io::Result<String> {
                self.queried = true;
                query("printf partial; exit 7")
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            }
            fn sync(&mut self) {}
            fn force_reboot(&mut self) {
                panic!("successful reboot command");
            }
            fn clear_volatile(&mut self) -> io::Result<()> {
                panic!("no start in place after a stop that failed");
            }
            fn remove_installed_skills(&mut self) -> io::Result<Vec<std::path::PathBuf>> {
                panic!("no wipe after a stop that failed");
            }
        }
        let root = Root::new();
        let marker = root.0.join("done");
        let data = root.0.join("personal");
        fs::write(&marker, b"done").unwrap();
        fs::write(&data, b"keep").unwrap();
        let plan = Plan {
            marker,
            journal: root.0.join("journal"),
            wipe: vec![data.clone()],
            stop: vec![
                "/bin/systemctl".into(),
                "stop".into(),
                "lampos.target".into(),
            ],
            reboot: vec!["/bin/systemctl".into(), "reboot".into()],
            detach: vec![],
        };
        let mut effects = EffectsWithQuery {
            rebooted: false,
            queried: false,
        };
        let steps = reset(&plan, &mut effects).unwrap();
        assert!(effects.queried && effects.rebooted);
        assert!(plan.journal.is_file());
        assert!(!plan.marker.exists());
        assert_eq!(fs::read(data).unwrap(), b"keep");
        assert_eq!(
            steps,
            vec![Step::Journal, Step::MarkerRemoved, Step::Rebooting]
        );
    }
}
