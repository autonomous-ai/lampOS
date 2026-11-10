//! Whether Harness is in use, read only, so a move to a newer pinned
//! Harness waits for a later round (#5957): a handed-over task worked on,
//! a sign-in pending, or a hand-over being sent.
//!
//! lamp-tasks keeps its store private (its own user, an exclusive lock), so
//! nothing here opens it. Two read-only signs are used instead:
//!
//! - `tasks/status`'s kept value on the bus ([`TaskStatus`]): a task in
//!   `tasks` or `unread` is being worked on. A value that can't be read as
//!   one is taken as in use. No value at all (lamp-tasks not run with
//!   `--progress`, or no bus) says nothing.
//! - The lamp's processes, under `<root>/proc`: a Harness command line
//!   (`harness` or its `cli.js`) followed by `login` (a sign-in by code,
//!   which runs while it is open), or `new` or `reply` (a hand-over or a
//!   follow-up being sent). Read as the round's root, never written.
use lamp_msgs::TaskStatus;
use std::{fs, io::Read, path::Path};

/// The most of one process's command line read.
const MOST_CMDLINE: u64 = 16 * 1024;
/// The most processes looked at.
const MOST_PROCESSES: usize = 32_768;

/// Why Harness is in use now, or none. `bus` is the bus's root; none reads
/// no bus.
pub fn in_use(root: &Path, bus: Option<&Path>) -> Option<&'static str> {
    if let Some(bus) = bus
        && let Some(why) = tasks_worked_on(bus)
    {
        return Some(why);
    }
    harness_running(&root.join("proc"))
}

/// [`in_use`] as the one-shot looks: the processes under `root` and the
/// bus at `LAMP_BUS_ROOT`, else [`lamp_bus::DEFAULT_ROOT`].
pub fn in_use_here(root: &Path) -> Option<&'static str> {
    let bus = std::env::var_os(lamp_bus::ROOT_VARIABLE)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| lamp_bus::DEFAULT_ROOT.into());
    in_use(root, Some(&bus))
}

fn tasks_worked_on(bus: &Path) -> Option<&'static str> {
    if !bus.join(TaskStatus::TOPIC).exists() {
        return None;
    }
    // A bus that can't be opened fails closed, as a failed read does.
    let Ok(bus) = lamp_bus::Bus::new(bus) else {
        return Some("lamp-tasks' status can't be read");
    };
    let kept = match bus.latest(TaskStatus::TOPIC) {
        Ok(Some(kept)) => kept,
        Ok(None) => return None,
        Err(_) => return Some("lamp-tasks' status can't be read"),
    };
    match serde_json::from_slice::<TaskStatus>(&kept.payload) {
        Ok(status) if status.tasks.is_empty() && status.unread.is_empty() => None,
        Ok(_) => Some("a handed-over task is being worked on"),
        Err(_) => Some("lamp-tasks' status can't be read"),
    }
}

/// A Harness `login`, `new` or `reply` among the processes under `proc`.
fn harness_running(proc: &Path) -> Option<&'static str> {
    let entries = fs::read_dir(proc).ok()?;
    for entry in entries.flatten().take(MOST_PROCESSES) {
        let name = entry.file_name();
        if !name
            .to_str()
            .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let mut line = Vec::new();
        let read = fs::File::open(entry.path().join("cmdline"))
            .and_then(|file| file.take(MOST_CMDLINE).read_to_end(&mut line));
        if read.is_err() {
            continue;
        }
        if let Some(why) = harness_command(&line) {
            return Some(why);
        }
    }
    None
}

/// What a command line (NUL-separated) says Harness is doing, when it is a
/// sign-in or a hand-over. Only flag options (`--json`, no value of their
/// own) are passed over before the verb, as Harness and lamp-tasks use them.
fn harness_command(cmdline: &[u8]) -> Option<&'static str> {
    let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
    let program = args.iter().position(|arg| {
        let name = arg.rsplit(|b| *b == b'/').next().unwrap_or(arg);
        name == b"harness" || name == b"cli.js"
    })?;
    // Options before the verb (`harness --json login`) are passed over.
    let verb = args[program + 1..]
        .iter()
        .find(|arg| !arg.starts_with(b"-"))?;
    match *verb {
        b"login" => Some("a Harness sign-in is pending"),
        b"new" | b"reply" => Some("a hand-over is being sent to Harness"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::Kind;

    #[test]
    fn a_sign_in_or_a_hand_over_is_harness_in_use() {
        let node = |rest: &str| {
            format!("/opt/node/bin/node\0/var/lib/lampos-tasks/.harness/cli/cli.js\0{rest}\0")
        };
        assert_eq!(
            harness_command(node("login\0--code\0--json").as_bytes()),
            Some("a Harness sign-in is pending")
        );
        for verb in ["new\0claude\0--machine\0m1", "reply\0s1"] {
            assert_eq!(
                harness_command(node(verb).as_bytes()),
                Some("a hand-over is being sent to Harness")
            );
        }
        assert_eq!(
            harness_command(b"/bin/sh\0/usr/local/bin/harness\0login\0--code\0"),
            Some("a Harness sign-in is pending")
        );
        for idle in [node("ps\0--json"), node("daemon"), node("version")] {
            assert_eq!(harness_command(idle.as_bytes()), None, "{idle:?}");
        }
        assert_eq!(harness_command(b"/usr/bin/vim\0new\0"), None);
        // Options before the verb are passed over (#5957's pre-review).
        assert_eq!(
            harness_command(node("--json\0-q\0login\0--code").as_bytes()),
            Some("a Harness sign-in is pending")
        );
        assert_eq!(
            harness_command(b"harness\0--verbose\0reply\0s1\0"),
            Some("a hand-over is being sent to Harness")
        );
        assert_eq!(harness_command(node("--json\0ps").as_bytes()), None);
    }

    #[test]
    fn the_processes_are_read_under_the_root() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(in_use(root.path(), None), None, "no /proc: nothing");
        let proc = root.path().join("proc");
        for (pid, line) in [("1", &b"/sbin/init\0"[..]), ("self", b"harness\0login\0")] {
            fs::create_dir_all(proc.join(pid)).unwrap();
            fs::write(proc.join(pid).join("cmdline"), line).unwrap();
        }
        assert_eq!(in_use(root.path(), None), None, "only numbered processes");
        fs::create_dir_all(proc.join("42")).unwrap();
        fs::write(proc.join("42/cmdline"), b"node\0/x/cli.js\0new\0claude\0").unwrap();
        assert_eq!(
            in_use(root.path(), None),
            Some("a hand-over is being sent to Harness")
        );
    }

    fn status(tasks: &str, unread: &str) -> String {
        format!(
            r#"{{"store_id":"0123456789abcdef0123456789abcdef","run":1,"tasks":{tasks}{unread}}}"#
        )
    }

    fn value(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap()
    }

    /// A bus that can't be opened (a relative root, refused by
    /// `Bus::new`) fails closed, as a failed read does.
    #[test]
    fn a_bus_that_cant_be_opened_is_harness_in_use() {
        let root = tempfile::tempdir().unwrap();
        let here = tempfile::tempdir_in(".").unwrap();
        let relative = Path::new(here.path().file_name().unwrap());
        fs::create_dir_all(relative.join(TaskStatus::TOPIC)).unwrap();
        assert_eq!(
            in_use(root.path(), Some(relative)),
            Some("lamp-tasks' status can't be read")
        );
    }

    #[test]
    fn a_task_on_tasks_status_is_harness_in_use() {
        let root = tempfile::tempdir().unwrap();
        let bus_root = tempfile::tempdir().unwrap();
        assert_eq!(in_use(root.path(), Some(bus_root.path())), None);
        let bus = lamp_bus::Bus::new(bus_root.path()).unwrap();
        let mut said = bus
            .publisher(TaskStatus::TOPIC, Kind::Latest, "tasks")
            .unwrap();
        said.publish_json(0, &value(&status("[]", ""))).unwrap();
        assert_eq!(
            in_use(root.path(), Some(bus_root.path())),
            None,
            "nothing worked on"
        );
        said.publish_json(0, &value(&status("[]", r#","unread":[3]"#)))
            .unwrap();
        assert_eq!(
            in_use(root.path(), Some(bus_root.path())),
            Some("a handed-over task is being worked on")
        );
        said.publish_json(0, &value("{}")).unwrap();
        assert_eq!(
            in_use(root.path(), Some(bus_root.path())),
            Some("lamp-tasks' status can't be read")
        );
    }
}
