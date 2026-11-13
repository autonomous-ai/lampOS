//! The manage helper's socket (`lamp-skills-manage.socket` on a lamp): what a
//! paired computer's `skill.push`, `skill.list` and `skill.remove` reach
//! through lamp-control's `--manage-socket`. systemd starts `lamp-skills
//! asked` for each connection, the connection its standard input and output;
//! lamp-sim does the same, so the REAL helper makes every check (the package's
//! hash and size, the skill's own load, the grants, the cap of 16, another
//! computer's names) and none is written again here.
//!
//! Differences from a lamp, all said: the helper runs as the developer, not
//! root; its pushed folders are the lamp folder's own (`asked --push-dir`,
//! `--dev-dir`); any peer may ask (`LAMP_SKILLS_ANY_ASKER=on`, as in the
//! helper's tests, since there is no systemd cgroup to name lamp-control);
//! the Store is off (no network install). Only push, list and remove: a
//! pushed skill is not run by lamp-sim.
use std::{
    io,
    os::{fd::OwnedFd, unix::net::UnixListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// At most this many helpers at once, as the socket unit's `MaxConnections`.
const MOST_AT_ONCE: usize = 4;
/// A helper is stopped after this long, as the unit's `RuntimeMaxSec`.
const RUNS_AT_MOST: Duration = Duration::from_secs(60);

/// Where the helper keeps things, all inside the lamp's folder.
pub struct Folders {
    /// The installed skills (`asked --dir`).
    pub skills: PathBuf,
    /// The helper's own state (`asked --state-dir`).
    pub state: PathBuf,
    /// Where lamp-control keeps a pushed package (`asked --push-dir`): the
    /// `push` folder of lamp-control's state folder.
    pub push: PathBuf,
    /// Where a pushed skill is kept (`asked --dev-dir`).
    pub dev: PathBuf,
}

impl Folders {
    /// The folders inside the lamp folder `dir`.
    pub fn in_dir(dir: &Path) -> Folders {
        Folders {
            skills: dir.join("skills"),
            state: dir.join("skills-state"),
            push: dir.join("state").join("push"),
            dev: dir.join("skills-dev"),
        }
    }
}

/// The helper's command: the real `lamp-skills asked` on `folders`.
fn helper(bin: &Path, folders: &Folders) -> Command {
    let mut command = crate::keys::command(bin);
    command
        .args(["asked", "--dir"])
        .arg(&folders.skills)
        .arg("--state-dir")
        .arg(&folders.state)
        .arg("--push-dir")
        .arg(&folders.push)
        .arg("--dev-dir")
        .arg(&folders.dev)
        .arg("--key-file")
        .arg(folders.state.join("no-store-key.pub"))
        .env("LAMP_SKILLS_ANY_ASKER", "on");
    command
}

/// Serves `socket` forever: one helper per connection, at most four at once.
pub fn serve(socket: &Path, bin: &Path, folders: Folders, log: &Path) -> io::Result<()> {
    for folder in [&folders.skills, &folders.state, &folders.push, &folders.dev] {
        std::fs::create_dir_all(folder)?;
    }
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    let folders = Arc::new(folders);
    let running = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming().flatten() {
        if running.fetch_add(1, Ordering::SeqCst) >= MOST_AT_ONCE {
            running.fetch_sub(1, Ordering::SeqCst);
            continue; // the connection is dropped, as a full socket unit does
        }
        let (bin, folders, running, log) = (
            bin.to_owned(),
            Arc::clone(&folders),
            Arc::clone(&running),
            log.to_owned(),
        );
        thread::spawn(move || {
            let _ = connection(stream.into(), &bin, &folders, &log);
            running.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

/// One connection: the helper with it as its input and output, stopped after
/// [`RUNS_AT_MOST`].
fn connection(stream: OwnedFd, bin: &Path, folders: &Folders, log: &Path) -> io::Result<()> {
    let out = stream.try_clone()?;
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?;
    let mut child: Child = helper(bin, folders)
        .stdin(Stdio::from(stream))
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if started.elapsed() >= RUNS_AT_MOST {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::{fs::PermissionsExt, net::UnixStream};

    fn scratch(name: &str) -> PathBuf {
        // Beside the test binary (under `target/`, never in git, and not a
        // noexec `/tmp`: the fake helper is run from here).
        let dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("manage-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The socket starts one helper per connection with the connection as its
    /// input and output, and the real helper's flags name the lamp folder's own
    /// folders.
    #[test]
    fn a_connection_reaches_the_helper_with_the_lamp_folders_on_its_command_line() {
        let _one_at_a_time = crate::FAKE_PROGRAM
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = scratch("helper");
        let bin = dir.join("lamp-skills");
        std::fs::write(
            &bin,
            "#!/bin/sh\nread line\necho \"{\\\"done\\\":\\\"$line|$*|$LAMP_SKILLS_ANY_ASKER\\\"}\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        // The socket in a short folder of its own: a Unix socket path holds
        // about 100 bytes, and a deep target folder would not bind (and the
        // connect loop below would wait for ever).
        let socket_dir = std::env::temp_dir().join(format!("lsm-{}", std::process::id()));
        std::fs::create_dir_all(&socket_dir).unwrap();
        let socket = socket_dir.join("m.sock");
        let (s, b, l) = (socket.clone(), bin.clone(), dir.join("log"));
        let folders = Folders::in_dir(&dir);
        let expect_dir = folders.skills.display().to_string();
        let served = thread::spawn(move || serve(&s, &b, folders, &l));
        let started = Instant::now();
        let mut stream = loop {
            if let Ok(stream) = UnixStream::connect(&socket) {
                break stream;
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the socket never came up: {:?}",
                served.is_finished().then(|| served.join())
            );
            thread::sleep(Duration::from_millis(20));
        };
        writeln!(stream, "{{\"list\":true}}").unwrap();
        let mut said = String::new();
        BufReader::new(stream).read_line(&mut said).unwrap();
        assert!(said.contains(r#"{"list":true}"#), "{said}");
        assert!(
            said.contains(&format!("asked --dir {expect_dir} --state-dir")),
            "{said}"
        );
        assert!(said.contains("--push-dir"), "{said}");
        assert!(said.contains("--dev-dir"), "{said}");
        assert!(said.ends_with("|on\"}\n"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&socket_dir);
    }
}
