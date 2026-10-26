//! The reset (#806): the reset button held 5 s and released, as Controls
//! publishes it on `controls/touch`, stops the lamp, wipes the person's data,
//! removes the setup marker and reboots into setup.
//!
//! A journal is written durably first, then the setup marker is removed, and
//! only then is the lamp stopped and its data wiped. So a power cut, or a stop
//! that fails, leaves a lamp with no marker and a journal: at the next start
//! the keeper sees no marker and starts only setup, and setup finishes the
//! wipe before anything else (`resume`), with nothing of the lamp running.

use lamp_msgs::{TOUCH_EVENT_TTL_US, TouchEvent, TouchGesture, TouchIntent, TouchSource};
use rustix::{
    fd::OwnedFd,
    fs::{AtFlags, Mode, OFlags},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
};

mod systemd;

/// The stock reset journal also fences Wi-Fi's independent rollback writer.
pub const PRODUCT_JOURNAL: &str = "/var/lib/lampos-setup/reset.json";
const PRODUCT_WIFI: &str = "/etc/wpa_supplicant/wpa_supplicant-wlan0.conf";
const WIFI_RESTORE_UNIT: &str = "lamp-setup-wifi-restore.service";

/// A pending reset must never restore an old network. Check before reading
/// any rollback bytes; reset removes those bytes before removing its journal.
/// Metadata errors refuse restoration too, rather than treating them as absence.
pub fn wifi_restore_allowed(journal: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(journal) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

/// What is never wiped, whatever a plan says: the software, its update trust,
/// and the system itself.
const PROTECTED: [&str; 12] = [
    "/opt/lampOS",
    "/etc/lampOS/update",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/boot",
    "/proc",
    "/sys",
    "/dev",
    "/root",
    "/home",
];

/// The written reset plan. Paths are absolute and plain (no `.` or `..`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Present once setup is done; a reset removes it.
    pub marker: PathBuf,
    /// Written before the lamp is stopped; removed once the wipe is done.
    pub journal: PathBuf,
    /// The person's data. A file is removed; a directory keeps itself, its
    /// owner and mode, and loses everything in it (systemd units name some
    /// of these directories and would not start without them).
    pub wipe: Vec<PathBuf>,
    /// Run before the wipe: the lamp's parts stopped (`lamp-keeper stop
    /// lamp`, or `systemctl stop lampos.target` on a lamp put on by
    /// `lamp-update install`, which has no keeper). If it fails, the lamp reboots without wiping, into setup,
    /// which finishes the wipe with nothing running.
    pub stop: Vec<String>,
    /// Run after the wipe.
    pub reboot: Vec<String>,
    /// How `watch-reset` hands the reset to a unit of its own, outside
    /// `lampos.target`, so stopping the lamp cannot stop the reset
    /// (`systemd-run --no-block --collect --unit=lamp-setup-wipe`). Empty:
    /// the reset runs in the watcher itself (tests).
    #[serde(default)]
    pub detach: Vec<String>,
}

/// The plan's fields, as [`read`] knows them.
pub const FIELDS: [&str; 6] = ["marker", "journal", "wipe", "stop", "reboot", "detach"];

/// The one field a newer build keeps something back from the reset in
/// (#1680, Third on #2060). This build knows no entry of it yet, so every
/// entry is dropped with a line and what it would keep is wiped: the
/// privacy-safe side, a pairing lost at worst.
pub const KEEP_BACK: &str = "keep_back";

/// What the person is told when a reset is refused (Third on #2060).
pub const SAY_REFUSED: &str = "Nothing was wiped. Update me, then reset again.";

/// Why a written plan is not acted on. Either way no reset runs: nothing is
/// journalled, the marker stays, nothing is wiped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A newer build's plan asks what this build cannot do: a `wipe` entry
    /// of a kind it does not know, or a field it does not know. The person
    /// is told ([`SAY_REFUSED`]) when they hold the button.
    Ahead(String),
    /// The plan does not read, or a known value is bad.
    Bad(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ahead(why) => write!(
                f,
                "the reset plan is a newer build's: {why}; a reset is refused and nothing is wiped (update the lamp, then reset again)"
            ),
            Self::Bad(why) => write!(f, "the plan: {why}"),
        }
    }
}

impl std::error::Error for Refused {}

/// Read a written plan, one version ahead (#1680, the rule agreed on #2060).
/// Anything to wipe is a plain path's text in `wipe`, which every build
/// wipes, so a newer plan's extra path is wiped here too (and still checked
/// against [`PROTECTED`]). A newer build keeps something back only in
/// [`KEEP_BACK`]: an entry this build does not understand there is dropped
/// with a returned line, so the reset wipes more, never less. Everything
/// else this build cannot do refuses the whole reset ([`Refused::Ahead`]):
/// a `wipe` entry that is not a path's text (wiping it as a path could wipe
/// what a newer build keeps, skipping it would keep the person's data), and
/// any other unknown field (it could be a step the reset needs). So a reset
/// is all or nothing, never a half-done one that reads as done.
pub fn read(bytes: &[u8]) -> Result<(Plan, Vec<String>), Refused> {
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| Refused::Bad(error.to_string()))?;
    let mut notes = Vec::new();
    if let Some(object) = value.as_object_mut() {
        if let Some(field) = object
            .keys()
            .find(|field| *field != KEEP_BACK && !FIELDS.contains(&field.as_str()))
        {
            return Err(Refused::Ahead(format!(
                "field `{field}` is not one this build knows"
            )));
        }
        if let Some(entries) = object.remove(KEEP_BACK) {
            let Some(entries) = entries.as_array() else {
                return Err(Refused::Bad(format!("`{KEEP_BACK}` is a list")));
            };
            for at in 1..=entries.len() {
                notes.push(format!(
                    "reset plan: {KEEP_BACK} entry {at} is not one this build understands; dropped, so what it keeps is wiped"
                ));
            }
        }
    }
    if let Some(entries) = value.get("wipe").and_then(|wipe| wipe.as_array())
        && let Some(at) = entries.iter().position(|entry| !entry.is_string())
    {
        return Err(Refused::Ahead(format!(
            "wipe entry {} is of a kind this build does not know",
            at + 1
        )));
    }
    let plan: Plan =
        serde_json::from_value(value).map_err(|error| Refused::Bad(error.to_string()))?;
    plan.check().map_err(Refused::Bad)?;
    Ok((plan, notes))
}

fn plain(path: &Path) -> bool {
    // Read the text itself: `Path::components` quietly drops a `.` in the
    // middle and a trailing `/`, which would let `/var/lib/./x` through.
    let Some(text) = path.to_str() else {
        return false;
    };
    text.len() > 1
        && text.starts_with('/')
        && text[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn command(argv: &[String]) -> bool {
    argv.first()
        .is_some_and(|program| plain(Path::new(program)))
        && argv.len() <= 16
        && argv.iter().all(|arg| !arg.is_empty() && arg.len() <= 256)
}

impl Plan {
    pub fn check(&self) -> Result<(), String> {
        if !plain(&self.marker) || !plain(&self.journal) {
            return Err("the marker and the journal are absolute, plain paths".into());
        }
        if self.wipe.is_empty() || self.wipe.len() > 64 {
            return Err("a plan wipes 1 to 64 paths".into());
        }
        for path in &self.wipe {
            if !plain(path) || path.components().count() < 4 {
                return Err(format!(
                    "{} is not an absolute plain path two levels below a top directory",
                    path.display()
                ));
            }
            if PROTECTED
                .iter()
                .any(|kept| path.starts_with(kept) || Path::new(kept).starts_with(path))
            {
                return Err(format!(
                    "{} is software or system, never wiped",
                    path.display()
                ));
            }
            if self.journal.starts_with(path) {
                return Err("the journal must not be inside a wiped path".into());
            }
        }
        if !command(&self.stop)
            || !command(&self.reboot)
            || (!self.detach.is_empty() && !command(&self.detach))
        {
            return Err("stop and reboot are commands with an absolute program".into());
        }
        Ok(())
    }
}

/// Whether a `controls/touch` event asks for the reset now. Only an enabled
/// Controls (not shadow), on this boot's clock, fresh, with a valid release
/// after 5 s or more of the reset button.
pub fn accept(event: &TouchEvent, clock_id: &str, now_us: u64) -> Result<(), &'static str> {
    event.check().map_err(|_| "invalid touch event")?;
    if event.source != TouchSource::Reset
        || event.gesture != TouchGesture::Hold
        || event.when_quiet != TouchIntent::FactoryReset
    {
        return Err("not the reset button held");
    }
    if !event.enabled {
        return Err("controls is in shadow");
    }
    if event.stamp.clock_id != clock_id {
        return Err("another boot's clock");
    }
    let age = now_us
        .checked_sub(event.stamp.at_us)
        .ok_or("from the future")?;
    if age >= TOUCH_EVENT_TTL_US {
        return Err("stale");
    }
    Ok(())
}

/// What the reset does outside the filesystem. Injected for tests.
pub trait Effects {
    fn run(&mut self, argv: &[String]) -> io::Result<()>;
    fn sync(&mut self);
    /// The last resort when the reboot command fails: a lamp left dark with
    /// no marker must still come back, into setup.
    fn force_reboot(&mut self);
}

pub struct System;

impl Effects for System {
    fn run(&mut self, argv: &[String]) -> io::Result<()> {
        if systemd_stop(argv) {
            return systemd::stop();
        }
        if matches!(argv, [program, verb, unit]
            if program == "/usr/bin/systemctl" && verb == "stop" && unit == WIFI_RESTORE_UNIT)
        {
            return systemd::stop_restore();
        }
        let status = Command::new(&argv[0]).args(&argv[1..]).status()?;
        if status.success() {
            // The keeper preserves its pause/rest protocol, but its target
            // job alone does not wait for the propagated service stop jobs.
            if keeper_stop(argv) {
                return systemd::stop();
            }
            Ok(())
        } else {
            Err(io::Error::other(format!("{} ended with {status}", argv[0])))
        }
    }
    fn sync(&mut self) {
        rustix::fs::sync();
    }
    fn force_reboot(&mut self) {
        rustix::fs::sync();
        #[cfg(target_os = "linux")]
        let _ = rustix::system::reboot(rustix::system::RebootCommand::Restart);
    }
}

fn systemd_stop(argv: &[String]) -> bool {
    matches!(argv, [program, verb, target]
        if matches!(program.as_str(), "/usr/bin/systemctl" | "/bin/systemctl")
            && verb == "stop" && target == "lampos.target")
}

fn keeper_stop(argv: &[String]) -> bool {
    matches!(argv, [program, verb, target]
        if program == "/usr/local/bin/lamp-keeper" && verb == "stop" && target == "lamp")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "step", content = "path")]
pub enum Step {
    Journal,
    Stopped,
    Wiped(PathBuf),
    MarkerRemoved,
    JournalRemoved,
    Rebooting,
}

fn sync_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => File::open(parent)?.sync_all(),
        None => Ok(()),
    }
}

fn write_journal(path: &Path) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    let _ = fs::remove_file(&temporary);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(b"{\"reset\":\"wiping\",\"version\":1}\n")?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    sync_parent(path)
}

fn remove_if_present(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => {
            sync_parent(path)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Open a directory one component at a time from `/`, following no link at
/// any level, so a part that swaps a directory for a link mid-wipe makes the
/// wipe fail rather than reach outside the plan.
fn open_dir(path: &Path) -> rustix::io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut fd = rustix::fs::open("/", flags, Mode::empty())?;
    for part in path.components().skip(1) {
        fd = rustix::fs::openat(&fd, part.as_os_str(), flags, Mode::empty())?;
    }
    Ok(fd)
}

fn is_dir(stat: &rustix::fs::Stat) -> bool {
    rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Directory
}

/// Remove everything inside the open directory, by handle.
fn empty(dir: &OwnedFd) -> io::Result<()> {
    let mut names = Vec::new();
    for entry in rustix::fs::Dir::read_from(dir)? {
        let name = entry?.file_name().to_owned();
        if name.as_bytes() != b"." && name.as_bytes() != b".." {
            names.push(name);
        }
    }
    for name in names {
        let stat = rustix::fs::statat(dir, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        if is_dir(&stat) {
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let child = rustix::fs::openat(dir, name.as_c_str(), flags, Mode::empty())?;
            empty(&child)?;
            rustix::fs::unlinkat(dir, name.as_c_str(), AtFlags::REMOVEDIR)?;
        } else {
            rustix::fs::unlinkat(dir, name.as_c_str(), AtFlags::empty())?;
        }
    }
    Ok(())
}

/// Remove a file, or everything inside a directory. Links are removed, never
/// followed, at the top, inside and on the way there.
pub fn wipe(path: &Path) -> io::Result<()> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::other("a wiped path has a parent and a name"));
    };
    let parent = match open_dir(parent) {
        Ok(parent) => parent,
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let stat = match rustix::fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if is_dir(&stat) {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let dir = rustix::fs::openat(&parent, name, flags, Mode::empty())?;
        empty(&dir)?;
        rustix::fs::fsync(&dir)?;
    } else {
        rustix::fs::unlinkat(&parent, name, AtFlags::empty())?;
        rustix::fs::fsync(&parent)?;
    }
    Ok(())
}

/// Begin a reset: journal, remove the marker, stop the lamp, wipe, reboot.
/// If the stop fails, reboot at once without wiping: setup finishes the wipe
/// at the next start, before anything of the lamp can run again.
pub fn reset(plan: &Plan, effects: &mut impl Effects) -> io::Result<Vec<Step>> {
    plan.check().map_err(io::Error::other)?;
    let mut steps = Vec::new();
    write_journal(&plan.journal)?;
    steps.push(Step::Journal);
    if remove_if_present(&plan.marker)? {
        steps.push(Step::MarkerRemoved);
    }
    effects.sync();
    if effects.run(&plan.stop).is_err() {
        reboot(plan, effects, &mut steps);
        return Ok(steps);
    }
    steps.push(Step::Stopped);
    if let Err(error) = finish(plan, effects, &mut steps) {
        // The journal and the missing marker bring the lamp back into setup,
        // which finishes the wipe; a lamp left dark here would wait for a hand.
        reboot(plan, effects, &mut steps);
        return Err(error);
    }
    reboot(plan, effects, &mut steps);
    Ok(steps)
}

fn reboot(plan: &Plan, effects: &mut impl Effects, steps: &mut Vec<Step>) {
    steps.push(Step::Rebooting);
    if effects.run(&plan.reboot).is_err() {
        effects.force_reboot();
    }
}

/// At start, before anything else: finish a reset a power cut interrupted.
/// The lamp's parts are not running yet. The stock plan additionally settles
/// the independent boot Wi-Fi rollback writer before wiping. There is no
/// second reboot. Returns the steps taken, empty when there was no reset.
pub fn resume(plan: &Plan, effects: &mut impl Effects) -> io::Result<Vec<Step>> {
    plan.check().map_err(io::Error::other)?;
    let mut steps = Vec::new();
    match fs::symlink_metadata(&plan.journal) {
        Ok(_) => {
            if systemd_stop(&plan.stop) || keeper_stop(&plan.stop) {
                effects.run(&[
                    "/usr/bin/systemctl".into(),
                    "stop".into(),
                    WIFI_RESTORE_UNIT.into(),
                ])?;
            }
            finish(plan, effects, &mut steps)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(steps)
}

fn finish(plan: &Plan, effects: &mut impl Effects, steps: &mut Vec<Step>) -> io::Result<()> {
    finish_with(plan, effects, steps, &mut wipe)
}

fn finish_with(
    plan: &Plan,
    effects: &mut impl Effects,
    steps: &mut Vec<Step>,
    erase: &mut impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    for path in &plan.wipe {
        if path == Path::new(PRODUCT_WIFI) {
            // Existing installed plans already name the canonical file. The
            // rollback and staging siblings belong to the same credential,
            // and must go before a later boot may run Wi-Fi restoration.
            wipe_wifi_siblings(path, erase)?;
        }
        erase(path)?;
        steps.push(Step::Wiped(path.clone()));
    }
    // Already gone after `reset`; this covers a journal found without one.
    if remove_if_present(&plan.marker)? {
        steps.push(Step::MarkerRemoved);
    }
    effects.sync();
    remove_if_present(&plan.journal)?;
    steps.push(Step::JournalRemoved);
    effects.sync();
    Ok(())
}

fn wipe_wifi_siblings(
    conf: &Path,
    erase: &mut impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    for extension in [
        "conf.lamp-previous",
        "conf.lamp-setup",
        "conf.lamp-setup-tmp",
    ] {
        erase(&conf.with_extension(extension))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::symlink,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "reset-wifi-{}-{}",
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
    fn pending_reset_refuses_wifi_restore_before_reading_old_bytes() {
        let root = Root::new();
        let journal = root.0.join("reset.json");
        assert!(wifi_restore_allowed(&journal).unwrap());
        fs::write(&journal, b"reset in progress").unwrap();
        assert!(!wifi_restore_allowed(&journal).unwrap());
        fs::remove_file(&journal).unwrap();
        symlink("missing", &journal).unwrap();
        assert!(!wifi_restore_allowed(&journal).unwrap());
        let not_directory = root.0.join("not-directory");
        fs::write(&not_directory, b"x").unwrap();
        assert!(wifi_restore_allowed(&not_directory.join("reset.json")).is_err());
    }

    #[test]
    fn finish_erases_stock_wifi_siblings_before_removing_the_journal() {
        struct Effects {
            journal: PathBuf,
            wifi: PathBuf,
            syncs: usize,
        }
        impl super::Effects for Effects {
            fn run(&mut self, _: &[String]) -> io::Result<()> {
                panic!("finish does not start commands")
            }
            fn sync(&mut self) {
                assert_eq!(fs::read_dir(&self.wifi).unwrap().count(), 0);
                self.syncs += 1;
                assert_eq!(self.journal.exists(), self.syncs == 1);
            }
            fn force_reboot(&mut self) {
                panic!("finish does not reboot")
            }
        }
        for fail_sibling in [false, true] {
            let root = Root::new();
            let directory = root.0.join("wifi");
            fs::create_dir(&directory).unwrap();
            let conf = directory.join("wpa_supplicant-wlan0.conf");
            let journal = root.0.join("reset.json");
            fs::write(&journal, b"reset in progress").unwrap();
            // A stopped helper may have just restored the old credential and
            // left its backup after a failed network rejoin.
            fs::write(&conf, b"synthetic restored credential").unwrap();
            for extension in [
                "conf.lamp-previous",
                "conf.lamp-setup",
                "conf.lamp-setup-tmp",
            ] {
                fs::write(conf.with_extension(extension), b"synthetic old credential").unwrap();
            }
            let calibration = root.0.join("calibration");
            fs::write(&calibration, b"keep").unwrap();
            let plan = Plan {
                marker: root.0.join("done"),
                journal: journal.clone(),
                wipe: vec![PRODUCT_WIFI.into()],
                stop: vec![
                    "/usr/bin/systemctl".into(),
                    "stop".into(),
                    "lampos.target".into(),
                ],
                reboot: vec!["/usr/bin/systemctl".into(), "reboot".into()],
                detach: vec![],
            };
            plan.check().unwrap();
            let mut effects = Effects {
                journal: journal.clone(),
                wifi: directory.clone(),
                syncs: 0,
            };
            let mut visited = Vec::new();
            let mut erase = |path: &Path| {
                let relative = path.strip_prefix("/etc/wpa_supplicant").unwrap();
                assert_eq!(relative.components().count(), 1);
                visited.push(path.to_path_buf());
                if fail_sibling && relative == Path::new("wpa_supplicant-wlan0.conf.lamp-setup") {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected sibling failure",
                    ));
                }
                wipe(&directory.join(relative))
            };
            let mut steps = Vec::new();
            let result = finish_with(&plan, &mut effects, &mut steps, &mut erase);
            if fail_sibling {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
                assert!(journal.exists() && conf.exists());
                assert!(!wifi_restore_allowed(&journal).unwrap());
                assert!(!steps.contains(&Step::JournalRemoved));
                assert_eq!(effects.syncs, 0);
            } else {
                result.unwrap();
                assert_eq!(effects.syncs, 2);
                assert!(wifi_restore_allowed(&journal).unwrap());
                assert_eq!(
                    visited,
                    [
                        Path::new(PRODUCT_WIFI).with_extension("conf.lamp-previous"),
                        Path::new(PRODUCT_WIFI).with_extension("conf.lamp-setup"),
                        Path::new(PRODUCT_WIFI).with_extension("conf.lamp-setup-tmp"),
                        PathBuf::from(PRODUCT_WIFI),
                    ]
                );
                assert!(steps.contains(&Step::JournalRemoved));
            }
            assert_eq!(fs::read(calibration).unwrap(), b"keep");
        }
    }
}
