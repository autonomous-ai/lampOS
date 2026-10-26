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
// An accepted Wi-Fi helper may spend its 120-second TimeoutStopSec in
// ExecStopPost. Discovery, all stop jobs and final verification share 180 s.
const STOP_LIMIT: Duration = Duration::from_secs(180);
const RESTORE: &str = "lamp-setup-wifi-restore.service";
const ACCEPT_SOCKETS: [&str; 3] = [
    "lamp-setup-change.socket",
    "lamp-setup-keys.socket",
    "lamp-setup-wifi.socket",
];

pub(super) fn stop() -> io::Result<()> {
    stop_with(&mut command, Instant::now() + STOP_LIMIT)
}

pub(super) fn stop_restore() -> io::Result<()> {
    stop_restore_with(&mut command, Instant::now() + STOP_LIMIT)
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
    properties_with_absent(bytes, expected, false)
}

fn properties_with_absent(
    bytes: &[u8],
    expected: &[&str],
    allow_absent: bool,
) -> io::Result<BTreeMap<String, String>> {
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
        || !(fields.get("LoadState").map(String::as_str) == Some("loaded")
            || (allow_absent
                && fields.get("LoadState").map(String::as_str) == Some("not-found")
                && fields.get("ActiveState").map(String::as_str) == Some("inactive")))
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
    show_with_absent(run, unit, field, until, false)
}

fn show_with_absent(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    unit: &str,
    field: &str,
    until: Instant,
    allow_absent: bool,
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
    if allow_absent {
        properties_with_absent(&output, &["LoadState", field], true)
    } else {
        properties(&output, &["LoadState", field])
    }
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

fn stop_units(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    units: &BTreeSet<String>,
    until: Instant,
) -> io::Result<()> {
    if units.is_empty() {
        return Ok(());
    }
    let mut args = vec!["stop".into(), "--".into()];
    args.extend(units.iter().cloned());
    time_left(until)?;
    let output = run(&args, until)?;
    time_left(until)?;
    if !output.is_empty() {
        return Err(invalid("unexpected systemctl stop output"));
    }
    Ok(())
}

fn inactive_members(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    units: &BTreeSet<String>,
    until: Instant,
) -> io::Result<()> {
    for unit in units {
        if show(run, unit, "ActiveState", until)?["ActiveState"] != "inactive" {
            return Err(invalid("reset member is not inactive after stop"));
        }
    }
    Ok(())
}

fn accepted_helper(socket: &str, helper: &str) -> bool {
    let Some(stem) = socket.strip_suffix(".socket") else {
        return false;
    };
    let Some(instance) = helper
        .strip_prefix(&format!("{stem}@"))
        .and_then(|value| value.strip_suffix(".service"))
    else {
        return false;
    };
    ACCEPT_SOCKETS.contains(&socket) && !instance.is_empty() && unit_name(helper)
}

fn accepted_helpers(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    units: &BTreeSet<String>,
    until: Instant,
) -> io::Result<BTreeSet<String>> {
    let mut helpers = BTreeSet::new();
    for socket in ACCEPT_SOCKETS
        .into_iter()
        .filter(|socket| units.contains(*socket))
    {
        let fields = show(run, socket, "Triggers", until)?;
        for helper in fields["Triggers"].split_ascii_whitespace() {
            if !accepted_helper(socket, helper) {
                return Err(invalid("unexpected accepted Setup helper unit"));
            }
            helpers.insert(helper.to_owned());
            if units.union(&helpers).count() + usize::from(!units.contains(RESTORE)) > MAX_UNITS {
                return Err(invalid("reset member and helper count exceeds bound"));
            }
        }
    }
    if units.union(&helpers).count() + usize::from(!units.contains(RESTORE)) > MAX_UNITS {
        return Err(invalid("reset member and helper count exceeds bound"));
    }
    Ok(helpers)
}

fn helper_state(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    helper: &str,
    until: Instant,
) -> io::Result<BTreeMap<String, String>> {
    // Only callers holding a validated concrete helper name or RESTORE may
    // accept garbage collection after that named unit's stop job finished.
    let fields = show_with_absent(run, helper, "ActiveState", until, true)?;
    if ![
        "inactive",
        "active",
        "activating",
        "deactivating",
        "reloading",
        "failed",
        "maintenance",
    ]
    .contains(&fields["ActiveState"].as_str())
    {
        return Err(invalid("unknown accepted Setup helper active state"));
    }
    Ok(fields)
}

fn inactive_helper(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    helper: &str,
    until: Instant,
) -> io::Result<()> {
    if helper_state(run, helper, until)?["ActiveState"] != "inactive" {
        return Err(invalid("accepted Setup helper is not inactive after stop"));
    }
    Ok(())
}

fn stop_restore_with(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    until: Instant,
) -> io::Result<()> {
    let before = helper_state(run, RESTORE, until)?;
    if before["LoadState"] == "not-found" {
        return time_left(until);
    }
    stop_units(run, &BTreeSet::from([RESTORE.to_owned()]), until)?;
    inactive_helper(run, RESTORE, until)
}

fn stop_with(
    run: &mut impl FnMut(&[String], Instant) -> io::Result<Vec<u8>>,
    until: Instant,
) -> io::Result<()> {
    let before = members(run, until)?;
    stop_units(run, &before, until)?;
    if members(run, until)? != before {
        return Err(invalid("reset membership changed while stopping"));
    }
    // Close and verify listeners first: a connection accepted before the stop
    // completed must be included when their concrete Triggers are discovered.
    inactive_members(run, &before, until)?;
    let helpers = accepted_helpers(run, &before, until)?;
    stop_units(run, &helpers, until)?;
    for helper in &helpers {
        inactive_helper(run, helper, until)?;
    }
    stop_restore_with(run, until)?;
    if members(run, until)? != before {
        return Err(invalid("reset membership changed while stopping helpers"));
    }
    inactive_members(run, &before, until)?;
    let remaining = accepted_helpers(run, &before, until)?;
    if !remaining.is_subset(&helpers) {
        return Err(invalid(
            "new accepted Setup helper appeared after listener stop",
        ));
    }
    for helper in remaining {
        inactive_helper(run, &helper, until)?;
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
        triggers: BTreeMap<String, String>,
        accepted_during_stop: BTreeMap<String, String>,
        helper_states: BTreeMap<String, &'static str>,
        helper_failure: Option<io::ErrorKind>,
        helper_gc: bool,
        helper_file: Option<std::path::PathBuf>,
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
                triggers: BTreeMap::new(),
                accepted_during_stop: BTreeMap::new(),
                helper_states: BTreeMap::new(),
                helper_failure: None,
                helper_gc: false,
                helper_file: None,
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
                let helpers: Vec<_> = args
                    .iter()
                    .filter(|unit| {
                        unit.as_str() == RESTORE
                            || ACCEPT_SOCKETS
                                .iter()
                                .any(|socket| accepted_helper(socket, unit))
                    })
                    .cloned()
                    .collect();
                if !helpers.is_empty() {
                    if let Some(kind) = self.helper_failure {
                        return Err(io::Error::new(kind, "injected helper stop failure"));
                    }
                    if let Some(path) = &self.helper_file {
                        std::fs::write(path, b"synthetic-helper-ExecStopPost-write")?;
                    }
                    for helper in helpers {
                        if self.helper_gc {
                            self.helper_states.remove(&helper);
                        } else {
                            self.helper_states.insert(helper, "inactive");
                        }
                    }
                }
                if args.iter().any(|unit| unit == TARGET) {
                    self.triggers.append(&mut self.accepted_during_stop);
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
            } else if field == "--property=Triggers" {
                assert!(
                    self.stopped,
                    "listeners stop before accepted helpers are discovered"
                );
                let value = self.triggers.get(unit).map(String::as_str).unwrap_or("");
                Ok(format!("LoadState=loaded\nTriggers={value}\n").into_bytes())
            } else if unit == RESTORE
                || ACCEPT_SOCKETS
                    .iter()
                    .any(|socket| accepted_helper(socket, unit))
            {
                if let Some(state) = self.helper_states.get(unit) {
                    Ok(format!("LoadState=loaded\nActiveState={state}\n").into_bytes())
                } else {
                    Ok(b"LoadState=not-found\nActiveState=inactive\n".to_vec())
                }
            } else {
                assert!(self.stopped);
                let state = self.active;
                Ok(format!("LoadState=loaded\nActiveState={state}\n").into_bytes())
            }
        }
        fn listener(&mut self, socket: &str) {
            self.graph
                .get_mut(TARGET)
                .unwrap()
                .push_str(&format!(" {socket}"));
            self.graph.insert(socket.into(), String::new());
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

    struct PersonalReset {
        root: std::path::PathBuf,
        personal: std::path::PathBuf,
        plan: super::super::Plan,
    }
    impl PersonalReset {
        fn new() -> Self {
            use std::{
                fs,
                os::unix::fs::DirBuilderExt,
                sync::atomic::{AtomicU64, Ordering},
            };
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "reset-accepted-helper-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
            fs::create_dir_all(root.join("personal")).unwrap();
            fs::create_dir_all(root.join("setup")).unwrap();
            let personal = root.join("personal/wifi.conf");
            fs::write(&personal, b"synthetic-private-network").unwrap();
            let marker = root.join("setup/done");
            fs::write(&marker, b"done").unwrap();
            let plan = super::super::Plan {
                marker,
                journal: root.join("setup/reset.json"),
                wipe: vec![root.join("personal")],
                stop: vec!["/usr/bin/systemctl".into(), "stop".into(), TARGET.into()],
                reboot: vec!["/usr/bin/systemctl".into(), "reboot".into()],
                detach: vec![],
            };
            Self {
                root,
                personal,
                plan,
            }
        }
        fn manager(&self) -> Manager {
            let mut manager = Manager::new();
            manager.listener(ACCEPT_SOCKETS[2]);
            let helper = "lamp-setup-wifi@accepted-before-close.service";
            manager
                .accepted_during_stop
                .insert(ACCEPT_SOCKETS[2].into(), helper.into());
            manager.helper_states.insert(helper.into(), "active");
            manager.helper_file = Some(self.personal.clone());
            manager
        }
    }
    impl Drop for PersonalReset {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    struct HelperEffects {
        manager: Manager,
        held: Option<(
            std::sync::mpsc::SyncSender<()>,
            std::sync::mpsc::Receiver<()>,
        )>,
        rebooted: bool,
    }
    impl super::super::Effects for HelperEffects {
        fn run(&mut self, argv: &[String]) -> io::Result<()> {
            if argv[1] == "stop" {
                stop_with(
                    &mut |args, until| {
                        if args[0] == "stop"
                            && args
                                .iter()
                                .any(|unit| accepted_helper(ACCEPT_SOCKETS[2], unit))
                            && let Some((entered, release)) = &self.held
                        {
                            entered.send(()).map_err(io::Error::other)?;
                            release
                                .recv_timeout(Duration::from_secs(5))
                                .map_err(io::Error::other)?;
                        }
                        self.manager.run(args, until)
                    },
                    Instant::now() + STOP_LIMIT,
                )
            } else {
                self.rebooted = true;
                Ok(())
            }
        }
        fn sync(&mut self) {}
        fn force_reboot(&mut self) {
            panic!("unexpected reboot fallback");
        }
    }

    #[test]
    fn held_accepted_helper_finishes_exec_stop_post_before_any_wipe() {
        let fixture = PersonalReset::new();
        let (entered, waiting) = std::sync::mpsc::sync_channel(1);
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let mut effects = HelperEffects {
            manager: fixture.manager(),
            held: Some((entered, released)),
            rebooted: false,
        };
        let plan = fixture.plan.clone();
        let worker = std::thread::spawn(move || {
            let result = super::super::reset(&plan, &mut effects);
            (result, effects)
        });
        let reached = waiting.recv_timeout(Duration::from_secs(5));
        let before = (
            fixture.personal.exists(),
            fixture.plan.marker.exists(),
            fixture.plan.journal.exists(),
        );
        let _ = release.send(());
        let (result, effects) = worker.join().unwrap();
        assert!(reached.is_ok(), "accepted helper stop was not reached");
        assert_eq!(
            before,
            (true, false, true),
            "no wipe before accepted helper cleanup"
        );
        result.unwrap();
        assert!(effects.rebooted);
        assert!(
            !fixture.personal.exists(),
            "ExecStopPost's final write must also be wiped"
        );
        assert!(!fixture.plan.journal.exists());
    }

    #[test]
    fn accepted_helper_stop_refusal_or_timeout_keeps_data_and_reset_journal() {
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::TimedOut] {
            let fixture = PersonalReset::new();
            let mut manager = fixture.manager();
            manager.helper_failure = Some(kind);
            let mut effects = HelperEffects {
                manager,
                held: None,
                rebooted: false,
            };
            let steps = super::super::reset(&fixture.plan, &mut effects).unwrap();
            assert!(effects.rebooted);
            assert_eq!(
                std::fs::read(&fixture.personal).unwrap(),
                b"synthetic-private-network"
            );
            assert!(fixture.plan.journal.exists() && !fixture.plan.marker.exists());
            assert!(
                !steps
                    .iter()
                    .any(|step| matches!(step, super::super::Step::Wiped(_)))
            );
        }
    }

    #[test]
    fn an_additional_trigger_after_helper_stop_refuses_success() {
        let mut manager = Manager::new();
        manager.listener(ACCEPT_SOCKETS[2]);
        let old = "lamp-setup-wifi@first.service";
        manager
            .triggers
            .insert(ACCEPT_SOCKETS[2].into(), old.into());
        manager.helper_states.insert(old.into(), "active");
        let result = stop_with(
            &mut |args, until| {
                let result = manager.run(args, until);
                if args[0] == "stop" && args.iter().any(|unit| unit == old) {
                    manager.triggers.insert(
                        ACCEPT_SOCKETS[2].into(),
                        "lamp-setup-wifi@late.service".into(),
                    );
                }
                result
            },
            Instant::now() + STOP_LIMIT,
        );
        assert!(result.is_err());
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
    fn accepted_connections_before_listener_stop_are_discovered_after_it() {
        let mut manager = Manager::new();
        for socket in ACCEPT_SOCKETS {
            manager.listener(socket);
            let helper = format!(
                "{}@accepted-1.service",
                socket.strip_suffix(".socket").unwrap()
            );
            manager
                .accepted_during_stop
                .insert(socket.into(), format!("{helper} {helper}"));
            manager.helper_states.insert(helper, "active");
        }
        manager.stop().unwrap();
        let initial_stop = manager
            .calls
            .iter()
            .position(|args| args[0] == "stop")
            .unwrap();
        for socket in ACCEPT_SOCKETS {
            let inactive = manager
                .calls
                .iter()
                .position(|args| {
                    args.last().map(String::as_str) == Some(socket)
                        && args.iter().any(|arg| arg == "--property=ActiveState")
                })
                .unwrap();
            let triggers = manager
                .calls
                .iter()
                .position(|args| {
                    args.last().map(String::as_str) == Some(socket)
                        && args.iter().any(|arg| arg == "--property=Triggers")
                })
                .unwrap();
            assert!(initial_stop < inactive && inactive < triggers);
        }
        let helper_stop = manager
            .calls
            .iter()
            .find(|args| {
                args[0] == "stop" && args.iter().any(|unit| unit.starts_with("lamp-setup-wifi@"))
            })
            .unwrap();
        assert_eq!(
            helper_stop.len(),
            5,
            "three deduplicated concrete helpers, stop and --"
        );
        assert!(
            manager
                .helper_states
                .values()
                .all(|state| *state == "inactive")
        );
    }

    #[test]
    fn listeners_must_be_inactive_and_trigger_names_must_match_their_socket() {
        let mut manager = Manager::new();
        manager.listener(ACCEPT_SOCKETS[0]);
        manager.active = "active";
        assert!(manager.stop().is_err());
        assert!(
            !manager
                .calls
                .iter()
                .any(|args| args.iter().any(|arg| arg == "--property=Triggers"))
        );
        for helper in [
            "lamp-setup-wifi@connection.service",
            "lamp-setup-change@.service",
            "unrelated.service",
            "lamp-setup-change@*.service",
        ] {
            let mut manager = Manager::new();
            manager.listener(ACCEPT_SOCKETS[0]);
            manager
                .triggers
                .insert(ACCEPT_SOCKETS[0].into(), helper.into());
            assert!(manager.stop().is_err(), "{helper}");
        }
    }

    #[test]
    fn named_helper_garbage_collection_is_safe_but_static_absence_is_not() {
        let mut manager = Manager::new();
        manager.listener(ACCEPT_SOCKETS[2]);
        let helper = "lamp-setup-wifi@connection.service";
        manager
            .triggers
            .insert(ACCEPT_SOCKETS[2].into(), helper.into());
        manager.helper_states.insert(helper.into(), "active");
        manager.helper_gc = true;
        manager.stop().unwrap();
        assert!(!manager.helper_states.contains_key(helper));
        assert!(
            properties(
                b"LoadState=not-found\nActiveState=inactive\n",
                &["LoadState", "ActiveState"]
            )
            .is_err()
        );
        assert!(
            properties_with_absent(
                b"LoadState=not-found\nActiveState=inactive\n",
                &["LoadState", "ActiveState"],
                true
            )
            .is_ok()
        );
        assert!(
            properties_with_absent(
                b"LoadState=not-found\nActiveState=active\n",
                &["LoadState", "ActiveState"],
                true
            )
            .is_err()
        );
    }

    #[test]
    fn accepted_helper_count_output_and_restore_state_are_bounded() {
        let mut manager = Manager::new();
        manager.listener(ACCEPT_SOCKETS[2]);
        manager.triggers.insert(
            ACCEPT_SOCKETS[2].into(),
            (0..MAX_UNITS)
                .map(|n| format!("lamp-setup-wifi@{n}.service"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        assert!(manager.stop().is_err());
        let mut manager = Manager::new();
        manager.listener(ACCEPT_SOCKETS[2]);
        manager
            .triggers
            .insert(ACCEPT_SOCKETS[2].into(), "x".repeat(MAX_OUTPUT + 1));
        assert!(manager.stop().is_err());
        let mut manager = Manager::new();
        manager.helper_states.insert(RESTORE.into(), "unknown");
        assert!(
            stop_restore_with(
                &mut |args, until| manager.run(args, until),
                Instant::now() + STOP_LIMIT
            )
            .is_err()
        );
        assert!(!manager.calls.iter().any(|args| args[0] == "stop"));
    }

    #[test]
    fn fixed_restore_is_stopped_or_truthfully_absent() {
        let mut manager = Manager::new();
        stop_restore_with(
            &mut |args, until| manager.run(args, until),
            Instant::now() + STOP_LIMIT,
        )
        .unwrap();
        assert!(!manager.calls.iter().any(|args| args[0] == "stop"));
        manager.helper_states.insert(RESTORE.into(), "active");
        stop_restore_with(
            &mut |args, until| manager.run(args, until),
            Instant::now() + STOP_LIMIT,
        )
        .unwrap();
        assert_eq!(manager.helper_states[RESTORE], "inactive");
        let mut manager = Manager::new();
        manager.helper_states.insert(RESTORE.into(), "active");
        manager.helper_failure = Some(io::ErrorKind::TimedOut);
        assert!(
            stop_restore_with(
                &mut |args, until| manager.run(args, until),
                Instant::now() + STOP_LIMIT
            )
            .is_err()
        );
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
