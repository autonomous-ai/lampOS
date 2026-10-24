use super::{Result, error};
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const CAP: usize = 1024 * 1024;
pub const QUERY: Duration = Duration::from_secs(3);

pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
}
fn group_absent(pid: Pid) -> bool {
    test_kill_process_group(pid) == Err(rustix::io::Errno::SRCH)
}
struct Cleanup {
    reaped: bool,
    group_absent: bool,
}
/// Signal only while the unreaped leader still reserves this PID/PGID identity.
/// Once reaped, probe only; a surviving/unknown group is unconfirmed cleanup.
fn reap(child: &mut Child, mut reaped: bool) -> Cleanup {
    let pid = Pid::from_child(child);
    if !reaped {
        let _ = kill_process_group(pid, Signal::TERM);
        let _ = kill_process_group(pid, Signal::KILL);
        let _ = child.kill();
    }
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if !reaped {
            reaped = child.try_wait().is_ok_and(|status| status.is_some());
        }
        let absent = reaped && group_absent(pid);
        if absent || Instant::now() >= until {
            return Cleanup {
                reaped,
                group_absent: absent,
            };
        }
        thread::sleep(Duration::from_millis(5));
    }
}
pub fn commands_cleaned(log: &[Value]) -> bool {
    !log.iter()
        .any(|row| row["unconfirmed_command_cleanup"] == true)
}
fn drain(reader: &mut impl Read, output: &mut Vec<u8>, until: Instant) -> Result<bool> {
    loop {
        if Instant::now() >= until {
            return Err("command drain exceeded original deadline".into());
        }
        let mut bytes = [0; 4096];
        match reader.read(&mut bytes) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                if output.len() + n > CAP {
                    return Err("command output exceeds fixed bound".into());
                }
                output.extend_from_slice(&bytes[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(error(e)),
        }
    }
}
/// Each command owns a group; failed waits retain explicit kill/reap/absence proof.
pub fn command(
    program: &Path,
    args: &[String],
    limit: Duration,
    log: &mut Vec<Value>,
) -> Result<Output> {
    // Bound stored diagnostics too; actual complete Records JSON is retained
    // separately after successful parsing, not reconstructed from this prefix.
    fn record(log: &mut Vec<Value>, value: Value) {
        let unconfirmed = value["group_absent"] == false;
        log.push(value);
        // Keep a permanent failure marker before the compaction region so no
        // later unit cleanup or bounded diagnostic can falsely erase it.
        if unconfirmed && commands_cleaned(log) {
            log.insert(0, json!({"unconfirmed_command_cleanup":true}));
        }
        while log.len() > 128 {
            let _ = log.remove(96);
        }
    }
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("SYSTEMD_COLORS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(error)?;
    let pid = child.id();
    let began = Instant::now();
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let mut leader_reaped = false;
    let result: Result<Option<i32>> = (|| {
        let mut stdout = child.stdout.take().ok_or("missing command stdout")?;
        let mut stderr = child.stderr.take().ok_or("missing command stderr")?;
        for fd in [
            &stdout as &dyn std::os::fd::AsFd,
            &stderr as &dyn std::os::fd::AsFd,
        ] {
            let flags = rustix::fs::fcntl_getfl(fd).map_err(error)?;
            rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK).map_err(error)?;
        }
        loop {
            let out_done = drain(&mut stdout, &mut stdout_bytes, began + limit)?;
            let err_done = drain(&mut stderr, &mut stderr_bytes, began + limit)?;
            // Do not reap an exited leader while a descendant holds a pipe:
            // its unreaped PID reserves our signal target through timeout.
            if out_done
                && err_done
                && let Some(status) = child.try_wait().map_err(error)?
            {
                leader_reaped = true;
                if !group_absent(Pid::from_child(&child)) {
                    return Err(
                        "command leader exited but owned group absence is unconfirmed".into(),
                    );
                }
                return Ok(status.code());
            }
            if began.elapsed() >= limit {
                return Err("external command exceeded original deadline".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    })();
    match result {
        Ok(code) => {
            let stdout = stdout_bytes;
            let stderr = stderr_bytes;
            record(
                log,
                json!({"program":program,"args":args,"pid":pid,"pgid":pid,
                "exit":code,"elapsed_us":began.elapsed().as_micros(),"timeout_ms":limit.as_millis(),
                "stdout":String::from_utf8_lossy(&stdout[..stdout.len().min(2048)]),"stderr":String::from_utf8_lossy(&stderr[..stderr.len().min(2048)]),
                "stdout_bytes":stdout.len(),"stderr_bytes":stderr.len(),
                "stdout_sha256":format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&stdout)),
                "stderr_sha256":format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&stderr)),
                "normal_exit":true,"reaped":true,"group_absent":true}),
            );
            Ok(Output { code, stdout })
        }
        Err(reason) => {
            let cleanup = reap(&mut child, leader_reaped);
            record(
                log,
                json!({"program":program,"args":args,"pid":pid,"pgid":pid,
                "error":reason,"reaped":cleanup.reaped,"group_absent":cleanup.group_absent,"elapsed_us":began.elapsed().as_micros(),
                "partial_stdout":String::from_utf8_lossy(&stdout_bytes[..stdout_bytes.len().min(2048)]),
                "partial_stderr":String::from_utf8_lossy(&stderr_bytes[..stderr_bytes.len().min(2048)]),
                "partial_stdout_bytes":stdout_bytes.len(),"partial_stderr_bytes":stderr_bytes.len()}),
            );
            Err(format!(
                "{reason}; owned command reaped={}, group_absent={}",
                cleanup.reaped, cleanup.group_absent
            ))
        }
    }
}
pub fn systemctl(args: &[&str], log: &mut Vec<Value>) -> Result<Output> {
    command(
        Path::new("/usr/bin/systemctl"),
        &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        QUERY,
        log,
    )
}
pub fn preflight(log: &mut Vec<Value>) -> Result<()> {
    if !rustix::process::geteuid().is_root() || std::env::consts::ARCH != "aarch64" {
        return Err("explicit release-pair qualification requires native AArch64 root".into());
    }
    let uname = command(Path::new("/usr/bin/uname"), &["-m".into()], QUERY, log)?;
    if uname.code != Some(0) || uname.stdout != b"aarch64\n" {
        return Err("kernel is not native AArch64".into());
    }
    if fs::read("/proc/1/comm").map_err(error)? != b"systemd\n"
        || fs::read_link("/proc/1/ns/pid").map_err(error)?
            != fs::read_link("/proc/self/ns/pid").map_err(error)?
    {
        return Err("real systemd PID1 and matching host PID namespace are required".into());
    }
    let version = systemctl(&["--version"], log)?;
    if version.code != Some(0) {
        return Err("real systemctl unavailable".into());
    }
    for unit in [
        "hal.service",
        "os-server.service",
        "led-boot.service",
        "led-shutdown.service",
    ] {
        let output = systemctl(&["is-active", "--quiet", unit], log)?;
        if !matches!(output.code, Some(3 | 4)) {
            return Err(format!(
                "legacy ownership not genuinely absent/inactive for {unit}; never stop it"
            ));
        }
    }
    // No fake proc/systemctl or guard injection: successful real Records reports
    // below are also required, and prove its complete double /proc scan ran.
    let fd = rustix::fs::openat2(
        rustix::fs::CWD,
        "/",
        rustix::fs::OFlags::DIRECTORY,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::empty(),
    )
    .map_err(error)?;
    drop(fd);
    Ok(())
}

pub struct Producer<'a> {
    pub binary: &'a Path,
    pub config: &'a Path,
    pub root: &'a Path,
    pub mute: &'a Path,
    pub bus: &'a Path,
    pub shared: bool,
}
pub struct Unit {
    pub name: String,
    pub invocation: Option<String>,
    pub group: Option<String>,
    pub stopped: bool,
    reserved: bool,
}
impl Unit {
    pub fn new(name: String) -> Self {
        Self {
            name,
            invocation: None,
            group: None,
            stopped: false,
            reserved: false,
        }
    }
    pub fn show(&self, log: &mut Vec<Value>) -> Result<BTreeMap<String, String>> {
        self.show_with_limit(log, QUERY)
    }
    fn show_with_limit(
        &self,
        log: &mut Vec<Value>,
        limit: Duration,
    ) -> Result<BTreeMap<String, String>> {
        if limit.is_zero() {
            return Err("original manager-query deadline exhausted".into());
        }
        let arguments = [
            "--system",
            "--no-pager",
            "--no-ask-password",
            "show",
            "--property=Id,LoadState,ActiveState,SubState,MainPID,ControlPID,InvocationID,ControlGroup,ExecStart,Type,NeedDaemonReload,Job",
            &self.name,
        ];
        let output = command(
            Path::new("/usr/bin/systemctl"),
            &arguments
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>(),
            limit.min(QUERY),
            log,
        )?;
        if !matches!(output.code, Some(0 | 4)) {
            return Err("cannot query owned fixture unit".into());
        }
        let text = std::str::from_utf8(&output.stdout).map_err(error)?;
        text.lines()
            .map(|line| {
                line.split_once('=')
                    .map(|(a, b)| (a.into(), b.into()))
                    .ok_or_else(|| "malformed actual systemd properties".into())
            })
            .collect()
    }
    pub fn reserve(&mut self, log: &mut Vec<Value>) -> Result<()> {
        let old = self.show(log)?;
        if old.get("LoadState").map(String::as_str) != Some("not-found") {
            return Err("unique fixture unit already exists; never replace or stop it".into());
        }
        self.reserved = true;
        Ok(())
    }
    pub fn start(&mut self, producer: Producer<'_>, log: &mut Vec<Value>) -> Result<PathBuf> {
        let Producer {
            binary,
            config,
            root,
            mute,
            bus,
            shared,
        } = producer;
        if !self.reserved {
            return Err("fixture unit was not reserved".into());
        }
        let until = Instant::now() + Duration::from_secs(10);
        let mut args = vec![
            "--quiet".into(),
            "--no-ask-password".into(),
            format!("--unit={}", self.name),
            "--property=Type=exec".into(),
            "--property=Restart=no".into(),
            "--property=RuntimeMaxSec=180s".into(),
            "--property=TimeoutStopSec=8s".into(),
            "--property=KillMode=control-group".into(),
            "--property=PrivateNetwork=yes".into(),
            "--property=DevicePolicy=closed".into(),
            format!("--setenv=LAMP_BUS_ROOT={}", bus.display()),
        ];
        if shared {
            args.push("--setenv=LAMP_SESSION_READ_GROUP=root".into());
        }
        args.extend([
            binary.display().to_string(),
            "serve".into(),
            config.display().to_string(),
            format!("{}/${{INVOCATION_ID}}", root.display()),
            "--controls-mute".into(),
            mute.display().to_string(),
            "--controls-bus-root".into(),
            bus.display().to_string(),
        ]);
        let result = command(
            Path::new("/usr/bin/systemd-run"),
            &args,
            until.saturating_duration_since(Instant::now()),
            log,
        )?;
        if result.code != Some(0) {
            return Err("real transient Voice unit did not start".into());
        }
        loop {
            let fields =
                self.show_with_limit(log, until.saturating_duration_since(Instant::now()))?;
            if let Some(group) = fields.get("ControlGroup").filter(|s| !s.is_empty()) {
                self.group = Some(group.clone());
            }
            if fields.get("ActiveState").map(String::as_str) != Some("active")
                || fields.get("SubState").map(String::as_str) != Some("running")
            {
                if Instant::now() >= until {
                    return Err("Voice did not remain running until metadata commit".into());
                }
            } else if let Some(id) = fields.get("InvocationID").filter(|id| valid_id(id)) {
                let pid: u32 = fields
                    .get("MainPID")
                    .ok_or("missing main PID")?
                    .parse()
                    .map_err(error)?;
                if pid == 0 {
                    return Err("active Voice lacks a process".into());
                }
                self.invocation = Some(id.clone());
                let session = root.join(id);
                let health = session.join("metadata/health.json");
                if let Ok(bytes) = super::inventory::read(&health, 64 * 1024)
                    && let Ok(h) = serde_json::from_slice::<Value>(&bytes)
                {
                    let events = super::inventory::read(
                        &session.join("metadata/events-0.jsonl"),
                        128 * 1024,
                    )
                    .unwrap_or_default();
                    let rows = events
                        .split(|b| *b == b'\n')
                        .filter_map(|l| serde_json::from_slice::<Value>(l).ok())
                        .collect::<Vec<_>>();
                    let segment = rows.first().is_some_and(|row| {
                        row["kind"] == "service_segment"
                            && row["generation"] == 0
                            && row["first_writer_event"] == 0
                    });
                    let first_event = rows.iter().any(|row| {
                        row["service_sequence"] == 1
                            && if shared {
                                row["shared_metadata"] == 1 && row["kind"] == "service_event"
                            } else {
                                row["kind"] == "run_start" && row["mode"] == "serve"
                            }
                    });
                    // Both releases persist health before incrementing the
                    // first event's written counter; a short muted run may
                    // truthfully leave that disk snapshot at zero. The actual
                    // first event bytes, not that lagging counter, prove write.
                    // 6449's shared projection intentionally removes run_start
                    // and mode, retaining its initial service_sequence instead.
                    if h["writer_finished"] == false
                        && h["health"]["writer_failed"] == false
                        && segment
                        && first_event
                    {
                        let child_path = format!("/proc/{pid}/task/{pid}/children");
                        if !fs::read_to_string(child_path)
                            .map_err(error)?
                            .trim()
                            .is_empty()
                        {
                            return Err("muted producer unexpectedly spawned a child worker".into());
                        }
                        let actual = fs::read_link(format!("/proc/{pid}/exe")).map_err(error)?;
                        if actual != binary {
                            return Err("real Voice executable changed".into());
                        }
                        log.push(json!({"producer_metadata_ready":true,"unit":self.name,
                            "shared_projection":shared,"first_service_sequence":1,
                            "persisted_written_events":h["health"]["written_events"],
                            "first_event_bytes_observed":true}));
                        return Ok(session);
                    }
                }
            }
            if Instant::now() >= until {
                return Err("actual muted writer metadata never committed".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn remains_live(&self, log: &mut Vec<Value>) -> Result<()> {
        let fields = self.show(log)?;
        if fields.get("ActiveState").map(String::as_str) != Some("active")
            || fields.get("SubState").map(String::as_str) != Some("running")
            || fields.get("MainPID").is_none_or(|p| p == "0")
            || fields.get("InvocationID") != self.invocation.as_ref()
        {
            return Err("protected actual producer did not remain the same live invocation".into());
        }
        Ok(())
    }
    pub fn stop(&mut self, log: &mut Vec<Value>) -> Result<()> {
        if self.stopped || !self.reserved {
            return Ok(());
        }
        let args = vec!["--no-ask-password".into(), "stop".into(), self.name.clone()];
        let stopped = command(
            Path::new("/usr/bin/systemctl"),
            &args,
            Duration::from_secs(10),
            log,
        );
        if !stopped.as_ref().is_ok_and(|s| s.code == Some(0)) {
            let _ = systemctl(
                &["kill", "--kill-who=all", "--signal=KILL", &self.name],
                log,
            );
        }
        let until = Instant::now() + Duration::from_secs(4);
        loop {
            let fields = self.show(log)?;
            if let Some(group) = fields.get("ControlGroup").filter(|s| !s.is_empty()) {
                self.group = Some(group.clone());
            }
            let absent = fields.get("LoadState").map(String::as_str) == Some("not-found");
            let inactive = matches!(
                fields.get("ActiveState").map(String::as_str),
                Some("inactive" | "failed")
            );
            let pid_zero = fields.get("MainPID").map(String::as_str) == Some("0");
            let control_zero = fields.get("ControlPID").map(String::as_str) == Some("0");
            let no_job = fields.get("Job").is_none_or(|j| j.is_empty() || j == "0");
            let empty_group = group_empty_at(
                Path::new("/sys/fs/cgroup"),
                &self.name,
                self.group.as_deref(),
            );
            if (absent || inactive) && pid_zero && control_zero && no_job && empty_group {
                self.stopped = true;
                return Ok(());
            }
            if Instant::now() >= until {
                return Err("owned unit/cgroup exit not proved; retain fixture and ledger".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
fn group_empty_at(mount: &Path, unit: &str, reported: Option<&str>) -> bool {
    if !unit.starts_with("lamp-records-pair-")
        || !unit.ends_with(".service")
        || !unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return false;
    }
    let expected = format!("/system.slice/{unit}");
    if reported.is_some_and(|group| !group.is_empty() && group != expected) {
        return false;
    }
    // An empty parent member list does not include child cgroups. Unified v2's
    // populated flag proves the whole subtree is empty before evidence removal.
    if !plain_directory(mount)
        || !plain_file(&mount.join("cgroup.controllers"))
        || !plain_directory(&mount.join("system.slice"))
    {
        return false;
    }
    let path = mount.join("system.slice").join(unit);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
        Ok(meta) if meta.is_dir() => {}
        _ => return false,
    }
    let Some(members) = group_text(&path.join("cgroup.procs"), 65_536) else {
        return false;
    };
    if !members.is_empty() {
        return false;
    }
    let Some(events) = group_text(&path.join("cgroup.events"), 4096) else {
        return false;
    };
    let mut fields = BTreeMap::new();
    for line in events.lines() {
        let mut words = line.split_whitespace();
        let (Some(key), Some(value), None) = (words.next(), words.next(), words.next()) else {
            return false;
        };
        if !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            || !value.bytes().all(|b| b.is_ascii_digit())
            || fields.insert(key, value).is_some()
        {
            return false;
        }
    }
    fields.get("populated") == Some(&"0")
}
fn plain_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}
fn plain_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
}
fn group_text(path: &Path, limit: u64) -> Option<String> {
    if !plain_file(path) {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && id.bytes().any(|b| b != b'0')
}

#[cfg(test)]
mod cgroup_tests {
    use super::*;
    use std::{
        os::unix::fs::symlink,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    const UNIT: &str = "lamp-records-pair-1-control-0.service";
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "records-cgroup-control-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::create_dir(path.join("system.slice")).unwrap();
            fs::write(path.join("cgroup.controllers"), "cpu memory\n").unwrap();
            Self(path)
        }
        fn group(&self, members: &str, events: &str) -> PathBuf {
            let path = self.0.join("system.slice").join(UNIT);
            fs::create_dir(&path).unwrap();
            fs::write(path.join("cgroup.procs"), members).unwrap();
            fs::write(path.join("cgroup.events"), events).unwrap();
            path
        }
        fn empty(&self) -> bool {
            group_empty_at(&self.0, UNIT, Some(&format!("/system.slice/{UNIT}")))
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn empty_parent_with_live_descendant_refuses_cleanup() {
        let tree = Tree::new();
        let group = tree.group("", "populated 1\nfrozen 0\n");
        fs::create_dir(group.join("child")).unwrap();
        fs::write(group.join("child/cgroup.procs"), "123\n").unwrap();
        assert!(!tree.empty());
        assert!(group.join("child/cgroup.procs").exists());
    }
    #[test]
    fn genuine_empty_subtree_and_absent_exact_group_allow_cleanup() {
        let tree = Tree::new();
        assert!(tree.empty());
        assert!(group_empty_at(&tree.0, UNIT, None));
        tree.group("", "populated 0\nfrozen 0\n");
        assert!(tree.empty());
        assert!(group_empty_at(&tree.0, UNIT, None));
        assert!(!group_empty_at(
            &tree.0,
            UNIT,
            Some("/system.slice/other.service")
        ));
    }
    #[test]
    fn missing_or_invalid_events_and_live_parent_refuse_cleanup() {
        let tree = Tree::new();
        let group = tree.group("", "populated 0\n");
        for events in [
            "",
            "frozen 0\n",
            "populated invalid\n",
            "populated 0\npopulated 0\n",
            "populated 0\npopulated 1\n",
            "populated 0\npopulated\t1\n",
            "populated 0\nfrozen invalid\n",
            "populated 0 extra\n",
        ] {
            fs::write(group.join("cgroup.events"), events).unwrap();
            assert!(!tree.empty(), "invalid events were accepted: {events:?}");
        }
        fs::remove_file(group.join("cgroup.events")).unwrap();
        assert!(!tree.empty());
        fs::write(group.join("cgroup.events"), "populated 0\n").unwrap();
        fs::write(group.join("cgroup.procs"), "123\n").unwrap();
        assert!(!tree.empty());
        fs::remove_file(group.join("cgroup.procs")).unwrap();
        assert!(!tree.empty());
    }
    #[test]
    fn absent_group_without_unified_v2_proof_refuses_cleanup() {
        let tree = Tree::new();
        fs::remove_file(tree.0.join("cgroup.controllers")).unwrap();
        assert!(!tree.empty());
    }
    #[test]
    fn symlinks_and_unexpected_group_paths_refuse_cleanup() {
        let tree = Tree::new();
        tree.group("", "populated 0\n");
        assert!(!group_empty_at(&tree.0, "../other.service", None));
        let group = tree.0.join("system.slice").join(UNIT);
        fs::rename(group.join("cgroup.events"), group.join("events-real")).unwrap();
        symlink("events-real", group.join("cgroup.events")).unwrap();
        assert!(!tree.empty());
        fs::remove_file(group.join("cgroup.events")).unwrap();
        fs::rename(group.join("events-real"), group.join("cgroup.events")).unwrap();
        fs::rename(&group, tree.0.join("saved")).unwrap();
        symlink("../saved", &group).unwrap();
        assert!(!tree.empty());
    }
    #[test]
    fn oversized_evidence_refuses_cleanup() {
        let tree = Tree::new();
        let group = tree.group("", &format!("populated 0\n{}", "x".repeat(4096)));
        assert!(!tree.empty());
        fs::write(group.join("cgroup.events"), "populated 0\n").unwrap();
        fs::write(group.join("cgroup.procs"), " ".repeat(65_537)).unwrap();
        assert!(!tree.empty());
    }
}
