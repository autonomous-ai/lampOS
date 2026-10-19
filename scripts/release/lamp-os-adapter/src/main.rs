//! OTA lifecycle adapter. All systemd changes are runtime-only and journalled.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitCode},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, String>;
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

#[derive(Deserialize)]
struct Manifest {
    format: u32,
    #[serde(default)]
    unit_id: String,
    #[serde(default)]
    profile_path: Option<String>,
    #[serde(default)]
    required_paths: Vec<String>,
    units: Vec<String>,
}
#[derive(Deserialize)]
struct Profile {
    unit_id: String,
    #[serde(default)]
    substitutions: BTreeMap<String, String>,
    #[serde(default)]
    required_paths: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct FileEntry {
    path: PathBuf,
    content: String,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    format: u32,
    service: String,
    units: Vec<String>,
    files: Vec<FileEntry>,
    marker: PathBuf,
    #[serde(default)]
    bus_group: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Health {
    ready: bool,
    version: String,
    pid: u32,
    waiting_for: Vec<String>,
    #[serde(default)]
    updated_at_ms: u128,
}
struct Layout {
    release: PathBuf,
    data: PathBuf,
    runtime: PathBuf,
    persistent: PathBuf,
    service: String,
}
trait Systemd {
    fn run(&self, verb: &str, units: &[String]) -> Result<()>;
    fn active(&self, unit: &str) -> bool;
    fn states(&self, units: &[String]) -> Result<BTreeMap<String, (bool, bool)>> {
        Ok(units
            .iter()
            .map(|unit| (unit.clone(), (true, self.active(unit))))
            .collect())
    }
}
fn bounded_systemctl(seconds: u32) -> Command {
    let mut command = Command::new("timeout");
    command.args(["--kill-after=1s", &format!("{seconds}s"), "systemctl"]);
    command
}
fn parse_states(text: &str, units: &[String]) -> Result<BTreeMap<String, (bool, bool)>> {
    let mut states = BTreeMap::new();
    for block in text.split("\n\n") {
        let fields: BTreeMap<_, _> = block
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();
        if let (Some(id), Some(load), Some(active)) = (
            fields.get("Id"),
            fields.get("LoadState"),
            fields.get("ActiveState"),
        ) {
            states.insert(
                (*id).to_owned(),
                (*load != "not-found", *active == "active"),
            );
        }
    }
    if units.iter().any(|unit| !states.contains_key(unit)) {
        return Err("incomplete systemd state response".into());
    }
    Ok(states)
}
struct RealSystemd;
impl Systemd for RealSystemd {
    fn states(&self, units: &[String]) -> Result<BTreeMap<String, (bool, bool)>> {
        if units.is_empty() {
            return Ok(BTreeMap::new());
        }
        let output = bounded_systemctl(3)
            .args(["show", "--property=Id,LoadState,ActiveState"])
            .args(units)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("systemd state query failed or timed out".into());
        }
        parse_states(&String::from_utf8_lossy(&output.stdout), units)
    }
    fn run(&self, verb: &str, units: &[String]) -> Result<()> {
        let mut command = bounded_systemctl(if verb == "stop" { 35 } else { 3 });
        command.arg(verb);
        if verb == "start" {
            command.arg("--no-block");
        }
        let status = command.args(units).status().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("systemctl {verb} failed or timed out"))
        }
    }
    fn active(&self, unit: &str) -> bool {
        bounded_systemctl(3)
            .args(["is-active", "--quiet", unit])
            .status()
            .is_ok_and(|s| s.success())
    }
}
fn keeper_command(release: &Path) -> Command {
    let mut command = Command::new(release.join("bin/lamp-keeper"));
    command
        .arg("serve")
        .env_remove("AUTONOMOUS_SERVICE_NAME")
        .env_remove("AUTONOMOUS_SERVICE_DATA");
    command
}
struct KeeperChild(Child);
impl KeeperChild {
    fn check(&mut self) -> Result<()> {
        match self.0.try_wait().map_err(|e| e.to_string())? {
            None => Ok(()),
            Some(status) => Err(format!("keeper exited unexpectedly: {status}")),
        }
    }
}
impl Drop for KeeperChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        unsafe {
            libc::kill(self.0.id() as libc::pid_t, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.0.kill();
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        eprintln!("lamp-os adapter: keeper did not exit within bounded shutdown; parent cgroup cleanup will finish it");
    }
}
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn fresh_health(mut health: Health, now: u128) -> Health {
    if health.updated_at_ms == 0 || now.abs_diff(health.updated_at_ms) > 3000 {
        health.ready = false;
    }
    health
}
fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    fs::create_dir_all(path.parent().ok_or("missing parent")?).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    use io::Write;
    let result = (|| {
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(path.parent().unwrap())?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e: io::Error| e.to_string())
}
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn valid_unit(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(base, suffix)| {
        valid_name(base)
            && matches!(suffix, "service" | "target" | "slice")
            && (base.starts_with("lamp-") || base.starts_with("lampos"))
    })
}
fn has_placeholder(mut text: &str) -> bool {
    while let Some(start) = text.find('@') {
        text = &text[start + 1..];
        if let Some(end) = text.find('@') {
            let key = &text[..end];
            if !key.is_empty()
                && key
                    .bytes()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
            {
                return true;
            }
        }
    }
    false
}
fn validate_accounts(content: &str) -> Result<()> {
    for line in content.lines() {
        let line = line.trim();
        let (group, value) = if let Some(value) = line.strip_prefix("User=") {
            (false, value)
        } else if let Some(value) = line.strip_prefix("Group=") {
            (true, value)
        } else {
            continue;
        };
        let name = std::ffi::CString::new(value).map_err(|_| "invalid account")?;
        let found = unsafe {
            if group {
                !libc::getgrnam(name.as_ptr()).is_null()
            } else {
                !libc::getpwnam(name.as_ptr()).is_null()
            }
        };
        if !found {
            return Err(format!("missing service account: {value}"));
        }
    }
    Ok(())
}
fn plan(layout: &Layout, hostname: &str) -> Result<Journal> {
    if !valid_name(&layout.service) {
        return Err("invalid service name".into());
    }
    let manifest: Manifest =
        serde_json::from_str(&read(&layout.release.join("package-manifest.json"))?)
            .map_err(|e| e.to_string())?;
    let mut profile = match &manifest.profile_path {
        Some(path) => {
            if path != "/etc/lampOS/package-profile.json" {
                return Err("invalid profile path".into());
            }
            use std::os::unix::fs::MetadataExt;
            let metadata =
                fs::symlink_metadata(path).map_err(|e| format!("missing device profile: {e}"))?;
            if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                return Err(
                    "device profile must be root-owned and not group/world writable".into(),
                );
            }
            serde_json::from_str::<Profile>(&read(Path::new(path))?).map_err(|e| e.to_string())?
        }
        None => Profile {
            unit_id: manifest.unit_id.clone(),
            substitutions: BTreeMap::new(),
            required_paths: Vec::new(),
        },
    };
    if manifest.format != 1 || profile.unit_id != hostname {
        return Err("package device identity does not match this hostname".into());
    }
    if profile
        .substitutions
        .get("UNIT_ID")
        .is_some_and(|id| id != &profile.unit_id)
    {
        return Err("profile UNIT_ID disagrees with hostname".into());
    }
    profile
        .substitutions
        .insert("UNIT_ID".into(), profile.unit_id.clone());
    for (key, value) in &profile.substitutions {
        if key == "RELEASE_ROOT"
            || key.is_empty()
            || !key
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
            || value
                .chars()
                .any(|c| c.is_control() || [37, 36, 92, 34, 39].contains(&(c as u32)))
        {
            return Err(format!("unsafe profile substitution: {key}"));
        }
    }
    for path in manifest
        .required_paths
        .iter()
        .chain(&profile.required_paths)
    {
        if !Path::new(path).is_absolute() || !Path::new(path).exists() {
            return Err(format!("missing prerequisite: {path}"));
        }
    }
    let unique: BTreeSet<_> = manifest.units.iter().collect();
    if unique.len() != manifest.units.len() || !manifest.units.iter().any(|u| u == "lampos.target")
    {
        return Err("invalid unit manifest".into());
    }
    let root = layout.release.to_str().ok_or("release path is not UTF-8")?;
    if root.contains(['\n', ' ', '%']) {
        return Err("unsupported release path".into());
    }
    let mut files = Vec::new();
    let mut services = Vec::new();
    for unit in &manifest.units {
        if !valid_unit(unit) || unit == &format!("{}.service", layout.service) {
            return Err(format!("invalid child unit: {unit}"));
        }
        for directory in [&layout.runtime, &layout.persistent] {
            if exists(&directory.join(unit)) || exists(&directory.join(format!("{unit}.d"))) {
                return Err(format!("unmanaged unit collision: {unit}"));
            }
        }
        let mut content =
            read(&layout.release.join("units").join(unit))?.replace("@RELEASE_ROOT@", root);
        for (key, value) in &profile.substitutions {
            content = content.replace(&format!("@{key}@"), value);
        }
        // Rendered units cannot retain template placeholders.
        if content
            .lines()
            .filter(|line| !line.trim_start().starts_with(['#', ';']))
            .any(has_placeholder)
        {
            return Err(format!("unresolved unit placeholder: {unit}"));
        }
        validate_accounts(&content)?;
        files.push(FileEntry {
            path: layout.runtime.join(unit),
            content,
        });
        if unit.ends_with(".service") || unit.ends_with(".target") {
            files.push(FileEntry {
                path: layout.runtime.join(format!("{unit}.d/99-ota-owner.conf")),
                content: format!(
                    "[Unit]\nBindsTo={0}.service\nPartOf={0}.service\nAfter={0}.service\n",
                    layout.service
                ),
            });
        }
        if unit.ends_with(".service") {
            services.push(unit.clone());
        }
    }
    if services.is_empty() {
        return Err("package has no child services".into());
    }
    files.push(FileEntry {
        path: layout.runtime.join("lampos.target.d/98-ota-parts.conf"),
        content: format!("[Unit]\nWants={}\n", services.join(" ")),
    });
    let bus_group = profile.substitutions.get("LAMP_USER").cloned().or_else(|| {
        files.iter().find_map(|entry| {
            entry.content.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("Group=")
                    .filter(|group| *group != "root")
                    .map(str::to_owned)
            })
        })
    });
    if let Some(group) = &bus_group {
        if !valid_name(group) {
            return Err("invalid bus group".into());
        }
        validate_accounts(&format!("Group={group}"))?;
        for entry in &mut files {
            if entry
                .path
                .to_string_lossy()
                .ends_with(".service.d/99-ota-owner.conf")
            {
                entry
                    .content
                    .push_str(&format!("[Service]\nSupplementaryGroups={group}\n"));
            }
        }
    }
    let marker = layout.data.join("adapter-active");
    for legacy in ["led-boot.service", "led-shutdown.service"] {
        let path = layout
            .runtime
            .join(format!("{legacy}.d/99-lamp-os-ota.conf"));
        if exists(&path)
            || exists(
                &layout
                    .persistent
                    .join(format!("{legacy}.d/99-lamp-os-ota.conf")),
            )
        {
            return Err(format!("legacy drop-in collision: {legacy}"));
        }
        files.push(FileEntry {
            path,
            content: format!("[Unit]\nConditionPathExists=!{}\n", marker.display()),
        });
    }
    Ok(Journal {
        format: 1,
        service: layout.service.clone(),
        units: manifest.units,
        files,
        marker,
        bus_group,
    })
}
fn journal_path(layout: &Layout) -> PathBuf {
    layout.data.join("adapter-journal.json")
}
fn validate_journal(layout: &Layout, journal: &Journal) -> Result<()> {
    if journal.format != 1
        || journal.service != layout.service
        || journal.marker != layout.data.join("adapter-active")
        || journal.units.iter().any(|u| !valid_unit(u))
    {
        return Err("invalid adapter journal".into());
    }
    for entry in &journal.files {
        let relative = entry
            .path
            .strip_prefix(&layout.runtime)
            .map_err(|_| "journal path outside runtime")?;
        if relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err("invalid journal path".into());
        }
    }
    Ok(())
}
fn cleanup(layout: &Layout, systemd: &impl Systemd) -> Result<()> {
    let path = journal_path(layout);
    if !exists(&path) {
        return Ok(());
    }
    let journal: Journal = serde_json::from_str(&read(&path)?).map_err(|e| e.to_string())?;
    validate_journal(layout, &journal)?;
    let requested: Vec<_> = journal
        .units
        .iter()
        .filter(|u| u.ends_with(".service") || u.ends_with(".target"))
        .cloned()
        .collect();
    let states = systemd.states(&requested)?;
    let units: Vec<_> = requested
        .into_iter()
        .filter(|u| states.get(u).is_some_and(|s| s.0))
        .collect();
    if !units.is_empty() {
        systemd.run("stop", &units)?;
    }
    // Never delete changed or externally replaced files.
    for entry in &journal.files {
        if exists(&entry.path)
            && (fs::symlink_metadata(&entry.path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
                || read(&entry.path)? != entry.content)
        {
            return Err(format!("owned file changed: {}", entry.path.display()));
        }
    }
    for entry in journal.files.iter().rev() {
        if exists(&entry.path) {
            fs::remove_file(&entry.path).map_err(|e| e.to_string())?;
        }
        if let Some(parent) = entry.path.parent().filter(|p| *p != layout.runtime) {
            let _ = fs::remove_dir(parent);
        }
    }
    systemd.run("daemon-reload", &[])?;
    if exists(&journal.marker) {
        fs::remove_file(&journal.marker).map_err(|e| e.to_string())?;
    }
    if exists(&layout.data.join("adapter-health.json")) {
        fs::remove_file(layout.data.join("adapter-health.json")).map_err(|e| e.to_string())?;
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}
fn expose_release(layout: &Layout) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    // Only the immutable, nonsecret artifact tree becomes traversable.
    // Do not expose the separate private DATA directory.
    let base = Path::new("/opt/autonomous-services");
    if !layout.release.starts_with(base) {
        return Ok(());
    }
    let relative = layout.release.strip_prefix(base).unwrap();
    let components: Vec<_> = relative.components().collect();
    if components.len() != 3
        || components[0].as_os_str() != layout.service.as_str()
        || components[1].as_os_str() != "releases"
        || components
            .iter()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("unexpected release layout".into());
    }
    let mut path = base.to_owned();
    for component in std::iter::once(None).chain(components.into_iter().map(Some)) {
        if let Some(component) = component {
            path.push(component.as_os_str());
        }
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err("release directory must be root-owned and not writable by others".into());
        }
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode(meta.mode() & 0o777 | 0o111),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn configure_bus(layout: &Layout, journal: &Journal) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if layout.runtime != Path::new("/run/systemd/system") {
        return Ok(());
    }
    let Some(group) = &journal.bus_group else {
        return Ok(());
    };
    let name = std::ffi::CString::new(group.as_str()).map_err(|_| "invalid bus group")?;
    let entry = unsafe { libc::getgrnam(name.as_ptr()) };
    if entry.is_null() {
        return Err("missing bus group".into());
    }
    let gid = unsafe { (*entry).gr_gid };
    let path = Path::new("/run/lampos-bus");
    if !exists(path) {
        fs::create_dir(path).map_err(|e| e.to_string())?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_dir() || metadata.uid() != 0 {
        return Err("bus root must be a root-owned directory, not a symlink".into());
    }
    let cpath = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    if unsafe { libc::chown(cpath.as_ptr(), 0, gid) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o2770)).map_err(|e| e.to_string())
}
fn install(layout: &Layout, hostname: &str, systemd: &impl Systemd) -> Result<Journal> {
    cleanup(layout, systemd)?;
    let journal = plan(layout, hostname)?;
    expose_release(layout)?;
    configure_bus(layout, &journal)?;
    write_atomic(
        &journal_path(layout),
        &serde_json::to_string(&journal).unwrap(),
    )?;
    write_atomic(&journal.marker, "active\n")?;
    for entry in &journal.files {
        write_atomic(&entry.path, &entry.content)?;
    }
    systemd.run("daemon-reload", &[])?;
    let legacy: Vec<String> = ["led-boot.service", "led-shutdown.service"]
        .iter()
        .filter(|unit| systemd.active(unit))
        .map(|unit| (*unit).to_owned())
        .collect();
    if !legacy.is_empty() {
        systemd.run("stop", &legacy)?;
    }
    if STOP.load(Ordering::SeqCst) {
        return Err("cancelled before starting children".into());
    }
    systemd.run("start", &["lampos.target".into()])?;
    Ok(journal)
}
fn run() -> Result<()> {
    let release = env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid executable location")?
        .to_owned();
    let layout = Layout {
        release,
        data: PathBuf::from(
            env::var("AUTONOMOUS_SERVICE_DATA").map_err(|_| "missing service data")?,
        ),
        runtime: "/run/systemd/system".into(),
        persistent: "/etc/systemd/system".into(),
        service: env::var("AUTONOMOUS_SERVICE_NAME").map_err(|_| "missing service name")?,
    };
    match env::args().nth(1).as_deref() {
        Some("cleanup") => cleanup(&layout, &RealSystemd),
        Some("health") => {
            let health = read(&layout.data.join("adapter-health.json"))
                .ok()
                .and_then(|text| serde_json::from_str::<Health>(&text).ok())
                .map(|health| fresh_health(health, now_ms()));
            println!(
                "{}",
                health
                    .map(|health| serde_json::to_string(&health).unwrap())
                    .unwrap_or_else(|| "{\"ready\":false,\"version\":\"\",\"pid\":0}".into())
            );
            Ok(())
        }
        Some("serve") => {
            if !read(Path::new("/usr/local/bin/software-update"))?
                .contains("# Service OTA cleanup protocol: 1")
            {
                return Err("software-update lacks required cleanup protocol; update bootstrap/updater first".into());
            }
            unsafe {
                libc::signal(
                    libc::SIGTERM,
                    stop_signal as *const () as libc::sighandler_t,
                );
                libc::signal(libc::SIGINT, stop_signal as *const () as libc::sighandler_t);
            }
            let hostname = read(Path::new("/etc/hostname"))?;
            let result = (|| {
                let journal = install(&layout, hostname.trim(), &RealSystemd)?;
                let version = read(&layout.release.join("VERSION"))?.trim().to_owned();
                let mut keeper = KeeperChild(
                    keeper_command(&layout.release)
                        .spawn()
                        .map_err(|e| format!("cannot start keeper: {e}"))?,
                );
                while !STOP.load(Ordering::SeqCst) {
                    keeper.check()?;
                    let services: Vec<_> = journal
                        .units
                        .iter()
                        .filter(|u| u.ends_with(".service"))
                        .cloned()
                        .collect();
                    let states = RealSystemd.states(&services);
                    let waiting_for: Vec<_> = services
                        .into_iter()
                        .filter(|u| {
                            !states
                                .as_ref()
                                .ok()
                                .and_then(|s| s.get(u))
                                .is_some_and(|s| s.1)
                        })
                        .collect();
                    let answer = Health {
                        ready: waiting_for.is_empty(),
                        version: version.clone(),
                        pid: std::process::id(),
                        waiting_for,
                        updated_at_ms: now_ms(),
                    };
                    write_atomic(
                        &layout.data.join("adapter-health.json"),
                        &serde_json::to_string(&answer).unwrap(),
                    )?;
                    thread::sleep(Duration::from_millis(200));
                }
                Ok(())
            })();
            let clean = cleanup(&layout, &RealSystemd);
            result.and(clean)
        }
        _ => Err("usage: service serve|health|cleanup".into()),
    }
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lamp-os adapter: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    struct Fixture(Layout, PathBuf);
    impl Fixture {
        fn new() -> Self {
            let base = env::temp_dir().join(format!(
                "lamp-adapter-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(base.join("release/units")).unwrap();
            let layout = Layout {
                release: base.join("release"),
                data: base.join("data"),
                runtime: base.join("runtime"),
                persistent: base.join("etc"),
                service: "lamp-os".into(),
            };
            fs::write(layout.release.join("package-manifest.json"), r#"{"format":1,"unit_id":"lamp-test","units":["lampos.target","lamp-voice.service"]}"#).unwrap();
            fs::write(
                layout.release.join("units/lampos.target"),
                "[Unit]\nDescription=Lamp\n",
            )
            .unwrap();
            fs::write(
                layout.release.join("units/lamp-voice.service"),
                "[Service]\nExecStart=@RELEASE_ROOT@/runtime/voice\n",
            )
            .unwrap();
            Self(layout, base)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.1);
        }
    }
    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
        fail_start: bool,
    }
    impl Systemd for Fake {
        fn run(&self, verb: &str, units: &[String]) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("{verb} {}", units.join(" ")));
            if self.fail_start && verb == "start" {
                Err("start failed".into())
            } else {
                Ok(())
            }
        }
        fn active(&self, _: &str) -> bool {
            false
        }
    }
    #[test]
    fn rejects_other_device_without_changes() {
        let f = Fixture::new();
        assert!(plan(&f.0, "lamp-other").is_err());
        assert!(!f.0.runtime.exists());
        assert!(!f.0.data.exists());
    }
    #[test]
    fn rejects_missing_prerequisite_without_changes() {
        let f = Fixture::new();
        let text = read(&f.0.release.join("package-manifest.json"))
            .unwrap()
            .replace(
                "\"format\":1",
                "\"format\":1,\"required_paths\":[\"/missing/lamp/credential\"]",
            );
        fs::write(f.0.release.join("package-manifest.json"), text).unwrap();
        assert!(plan(&f.0, "lamp-test").is_err());
        assert!(!f.0.runtime.exists());
    }
    #[test]
    fn refuses_existing_unit_and_dropin() {
        let f = Fixture::new();
        fs::create_dir_all(f.0.persistent.join("lamp-voice.service.d")).unwrap();
        assert!(plan(&f.0, "lamp-test").is_err());
    }
    #[test]
    fn renders_and_binds_every_child_without_persistent_changes() {
        let f = Fixture::new();
        let fake = Fake::default();
        install(&f.0, "lamp-test", &fake).unwrap();
        assert!(!f.0.persistent.exists());
        assert!(read(&f.0.runtime.join("lamp-voice.service"))
            .unwrap()
            .contains(f.0.release.to_str().unwrap()));
        assert!(
            read(&f.0.runtime.join("lamp-voice.service.d/99-ota-owner.conf"))
                .unwrap()
                .contains("BindsTo=lamp-os.service")
        );
        assert!(read(&f.0.runtime.join("lampos.target.d/98-ota-parts.conf"))
            .unwrap()
            .contains("Wants=lamp-voice.service"));
        assert!(!fake
            .calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("mask") || c.starts_with("enable")));
        cleanup(&f.0, &fake).unwrap();
        cleanup(&f.0, &fake).unwrap();
        assert!(!journal_path(&f.0).exists());
        assert!(!f.0.runtime.join("lamp-voice.service").exists());
    }
    #[test]
    fn start_failure_can_be_cleaned_from_journal() {
        let f = Fixture::new();
        let fake = Fake {
            fail_start: true,
            ..Fake::default()
        };
        assert!(install(&f.0, "lamp-test", &fake).is_err());
        assert!(journal_path(&f.0).exists());
        cleanup(&f.0, &fake).unwrap();
        assert!(!f.0.runtime.join("lampos.target").exists());
    }
    #[test]
    fn changed_file_preserved_and_cleanup_failure_visible() {
        let f = Fixture::new();
        let fake = Fake::default();
        install(&f.0, "lamp-test", &fake).unwrap();
        fs::write(f.0.runtime.join("lamp-voice.service"), "external edit").unwrap();
        assert!(cleanup(&f.0, &fake).is_err());
        assert_eq!(
            read(&f.0.runtime.join("lamp-voice.service")).unwrap(),
            "external edit"
        );
        assert!(journal_path(&f.0).exists());
    }
    #[test]
    fn recovery_reinstalls_owned_units_after_interrupted_install() {
        let f = Fixture::new();
        let fake = Fake::default();
        install(&f.0, "lamp-test", &fake).unwrap();
        install(&f.0, "lamp-test", &fake).unwrap();
        cleanup(&f.0, &fake).unwrap();
    }
    #[test]
    fn unresolved_template_is_not_published() {
        let f = Fixture::new();
        fs::write(
            f.0.release.join("units/lamp-voice.service"),
            "User=@LAMP_USER@\n",
        )
        .unwrap();
        assert!(plan(&f.0, "lamp-test").is_err());
    }
    #[test]
    fn journal_cannot_remove_outside_runtime() {
        let f = Fixture::new();
        let mut journal = plan(&f.0, "lamp-test").unwrap();
        journal.files[0].path = f.0.runtime.join("../outside");
        assert!(validate_journal(&f.0, &journal).is_err());
    }
    #[test]
    fn camera_usb_at_sign_is_not_a_template() {
        assert!(!has_placeholder("--usb 01da:5875@3-1.1.4"));
        assert!(has_placeholder("--usb 01da:5875@3-1.1.4 User=@LAMP_USER@"));
    }
    #[test]
    fn nonexistent_account_rejected_before_unit_changes() {
        let f = Fixture::new();
        fs::write(
            f.0.release.join("units/lamp-voice.service"),
            "[Service]\nUser=lamp-invalid-missing-account-xyz\n",
        )
        .unwrap();
        assert!(plan(&f.0, "lamp-test").is_err());
        assert!(!f.0.runtime.exists());
    }
    #[test]
    fn journal_before_first_file_is_recoverable() {
        let f = Fixture::new();
        let fake = Fake::default();
        let journal = plan(&f.0, "lamp-test").unwrap();
        write_atomic(
            &journal_path(&f.0),
            &serde_json::to_string(&journal).unwrap(),
        )
        .unwrap();
        cleanup(&f.0, &fake).unwrap();
        assert!(!journal_path(&f.0).exists());
    }
    #[test]
    fn stale_health_is_never_ready() {
        let health = Health {
            ready: true,
            version: "1".into(),
            pid: 123,
            waiting_for: vec![],
            updated_at_ms: 1000,
        };
        assert!(!fresh_health(health, 4001).ready);
        let health = Health {
            ready: true,
            version: "1".into(),
            pid: 123,
            waiting_for: vec![],
            updated_at_ms: 1000,
        };
        assert!(fresh_health(health, 1100).ready);
    }
    #[test]
    fn batch_states_require_every_unit_and_distinguish_missing() {
        let units = vec!["lamp-voice.service".into(), "lampos.target".into()];
        let text = "Id=lamp-voice.service\nLoadState=loaded\nActiveState=active\n\nId=lampos.target\nLoadState=not-found\nActiveState=inactive\n";
        let states = parse_states(text, &units).unwrap();
        assert_eq!(states["lamp-voice.service"], (true, true));
        assert_eq!(states["lampos.target"], (false, false));
        assert!(parse_states("", &units).is_err());
    }
    #[test]
    fn systemctl_invocations_have_timeout_and_forced_kill_bound() {
        let command = bounded_systemctl(3);
        let args: Vec<_> = command.get_args().map(|v| v.to_str().unwrap()).collect();
        assert_eq!(args, vec!["--kill-after=1s", "3s", "systemctl"]);
    }
    #[test]
    fn keeper_monitors_without_recursive_installation() {
        let command = keeper_command(Path::new("/package"));
        assert_eq!(command.get_program(), "/package/bin/lamp-keeper");
        assert_eq!(command.get_args().collect::<Vec<_>>(), vec!["serve"]);
        let env: BTreeMap<_, _> = command.get_envs().collect();
        assert_eq!(
            env.get(std::ffi::OsStr::new("AUTONOMOUS_SERVICE_NAME")),
            Some(&None)
        );
        assert_eq!(
            env.get(std::ffi::OsStr::new("AUTONOMOUS_SERVICE_DATA")),
            Some(&None)
        );
    }
    #[test]
    fn failed_systemd_inspection_preserves_cleanup_journal() {
        struct Broken;
        impl Systemd for Broken {
            fn run(&self, _: &str, _: &[String]) -> Result<()> {
                panic!("must not mutate after inspection failure")
            }
            fn active(&self, _: &str) -> bool {
                false
            }
            fn states(&self, _: &[String]) -> Result<BTreeMap<String, (bool, bool)>> {
                Err("timeout".into())
            }
        }
        let f = Fixture::new();
        install(&f.0, "lamp-test", &Fake::default()).unwrap();
        assert!(cleanup(&f.0, &Broken).is_err());
        assert!(journal_path(&f.0).exists());
        assert!(f.0.runtime.join("lamp-voice.service").exists());
    }
}
