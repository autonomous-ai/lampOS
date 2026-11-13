//! `service prepare` as the updater runs it: the real adapter binary placed in
//! a release folder beside a fake keeper.
use std::{
    env, fs,
    io::ErrorKind,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Duration,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Release(PathBuf);
impl Release {
    fn new(keeper: Option<&str>) -> Self {
        let base = env::temp_dir().join(format!(
            "lamp-adapter-prepare-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(base.join("release/bin")).unwrap();
        fs::create_dir_all(base.join("data")).unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_lamp-os-adapter"),
            base.join("release/bin/service"),
        )
        .unwrap();
        if let Some(script) = keeper {
            let path = base.join("release/bin/lamp-keeper");
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self(base)
    }
    fn prepare(&self) -> (std::process::Output, u32) {
        // A parallel test thread that forks between our write and our exec briefly
        // holds a copy of the write fd, so exec fails with ETXTBSY until its child
        // execs. Retry for up to ~1s; the window is microseconds, so this is ample.
        let mut attempt = 0;
        let child = loop {
            let spawned = Command::new(self.0.join("release/bin/service"))
                .arg("prepare")
                .current_dir(self.0.join("data"))
                .env("AUTONOMOUS_SERVICE_NAME", "lamp-os")
                .env("AUTONOMOUS_SERVICE_DATA", self.0.join("data"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn();
            match spawned {
                Err(e) if e.kind() == ErrorKind::ExecutableFileBusy && attempt < 100 => {
                    attempt += 1;
                    thread::sleep(Duration::from_millis(10));
                }
                other => break other.unwrap(),
            }
        };
        let pid = child.id();
        (child.wait_with_output().unwrap(), pid)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn keeper(exit: u8, record: &Path) -> String {
    format!(
        "#!/bin/sh\nprintf '%s|%s|%s|%s' \"$*\" \"${{AUTONOMOUS_SERVICE_NAME-unset}}\" \"${{AUTONOMOUS_SERVICE_DATA-unset}}\" \"$$\" > '{}'\nexit {exit}\n",
        record.display()
    )
}

#[test]
fn prepare_becomes_the_keeper_and_returns_its_status() {
    for exit in [0u8, 7] {
        let record = env::temp_dir().join(format!(
            "lamp-adapter-prepare-record-{}-{}",
            std::process::id(),
            exit
        ));
        let release = Release::new(Some(&keeper(exit, &record)));
        let (output, pid) = release.prepare();
        assert_eq!(output.status.code(), Some(exit as i32), "{output:?}");
        let seen = fs::read_to_string(&record).unwrap();
        let _ = fs::remove_file(&record);
        // Same PID: the adapter exec'd the keeper rather than supervising a child.
        assert_eq!(seen, format!("prepare|unset|unset|{pid}"));
        assert!(!release.path("data/adapter-health.json").exists());
    }
}

#[test]
fn prepare_without_keeper_fails_clearly() {
    let release = Release::new(None);
    let (output, _) = release.prepare();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot run keeper prepare"));
}

#[test]
fn keeper_without_prepare_command_fails() {
    // A keeper older than lamp-voice #1094 treats `prepare` as a serve option
    // and exits with usage status 2; the updater must see that failure.
    let release = Release::new(Some("#!/bin/sh\necho usage >&2\nexit 2\n"));
    let (output, _) = release.prepare();
    assert_eq!(output.status.code(), Some(2));
}
