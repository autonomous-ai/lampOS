//! Test-only isolation for the two held memory acknowledgement scenarios.
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const CHILD: &str = "LAMP_MEMORY_NO_WAIT_TEST_CHILD";
const ROOT: &str = "LAMP_MEMORY_NO_WAIT_TEST_ROOT";
const WATCHDOG: Duration = Duration::from_secs(15);

struct Probe {
    child: Option<Child>,
    directory: crate::process::SessionDirectory,
    passed: bool,
}

impl Drop for Probe {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            // The scenario starts threads, not descendant processes. Keep the
            // private socket and logs if this owned child's cleanup fails.
            let cleanup = (|| {
                if child.try_wait()?.is_some() {
                    return Ok(());
                }
                if let Err(error) = child.kill() {
                    // Exit can race kill; observe and reap that child too.
                    if child.try_wait()?.is_none() {
                        return Err(error);
                    }
                    return Ok(());
                }
                child.wait().map(|_| ())
            })();
            if let Err(error) = cleanup {
                eprintln!("memory no-wait child cleanup failed: {error}");
                return;
            }
        }
        let _ = fs::remove_file(self.directory.path.join("memory.sock"));
        if !self.passed {
            eprintln!(
                "memory no-wait failure logs retained at {}",
                self.directory.path.display()
            );
            return;
        }
        for name in ["stdout", "stderr"] {
            let _ = fs::remove_file(self.directory.path.join(name));
        }
    }
}

fn output(path: &Path) -> String {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .unwrap()
        .take(65_537)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        bytes.len() <= 65_536,
        "memory scenario output exceeded bound"
    );
    String::from_utf8(bytes).unwrap()
}

pub(super) fn run(name: &str, scenario: impl FnOnce(&Path)) {
    if std::env::var(CHILD).ok().as_deref() == Some(name) {
        let root = std::env::var_os(ROOT).expect("memory scenario root");
        scenario(Path::new(&root));
        return;
    }

    let directory = crate::process::SessionDirectory::create().unwrap();
    let stdout = fs::File::create(directory.path.join("stdout")).unwrap();
    let stderr = fs::File::create(directory.path.join("stderr")).unwrap();
    let mut probe = Probe {
        child: None,
        directory,
        passed: false,
    };
    probe.child = Some(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--test-threads=1", "--nocapture"])
            .env(CHILD, name)
            .env(ROOT, &probe.directory.path)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + WATCHDOG;
    let status = loop {
        if let Some(status) = probe.child.as_mut().unwrap().try_wait().unwrap() {
            probe.child = None;
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "foreground memory scenario waited for a held acknowledgement: {name}"
        );
        thread::sleep(Duration::from_millis(5));
    };
    let stdout = output(&probe.directory.path.join("stdout"));
    let stderr = output(&probe.directory.path.join("stderr"));
    assert!(status.success(), "{name}: {status}\n{stdout}\n{stderr}");
    assert!(
        stdout
            .lines()
            .any(|line| line == format!("test {name} ... ok"))
            && stdout.lines().any(|line| line
                .starts_with("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;")),
        "exactly one successful child case is required: {stdout}"
    );
    probe.passed = true;
}
