//! The reset (#806): the reset button held 5 s and released, as Controls
//! publishes it on `controls/touch`, stops the lamp, wipes the person's data,
//! removes the setup marker and starts setup again: in place on a product lamp
//! (#4500, [`stop_and_wait_keeping`]), else by the plan's reboot.
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

pub(crate) mod query;

/// The product plan's journal, read by Wi-Fi's restore of an old network
/// ([`wifi_restore_allowed`]).
pub const PRODUCT_JOURNAL: &str = "/var/lib/lampos-setup/reset.json";

/// The Wi-Fi file whose rollback and staging copies beside it are wiped with
/// it, matched by name so that a plan installed before them wipes them too.
const WIFI_FILE: &str = "wpa_supplicant-wlan0.conf";

/// What Wi-Fi keeps beside its file: the old network a change may put back
/// (`.conf.lamp-previous`, which the boot's `lamp-setup-wifi-restore.service`
/// puts back), and setup's staging copies (Third on #3579).
const WIFI_COPIES: [&str; 3] = [
    "conf.lamp-previous",
    "conf.lamp-setup",
    "conf.lamp-setup-tmp",
];

/// The extension setup's staging copy of a file takes (`serve::stage`).
const STAGED: &str = "lamp-setup-tmp";

/// Whether Wi-Fi may put an old network back (`lamp-setup wifi-restore`, at
/// boot and after a change helper's stop): never while a reset is
/// journalled, so a reset's wipe of the Wi-Fi cannot be undone from the
/// rollback copy, which the reset wipes before it removes its journal
/// (Third on #3579). A journal that cannot be read counts as there.
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
    /// `lamp-update install`, which has no keeper, run with each part of
    /// the target named: [`stop_and_wait`]). If it fails, the lamp reboots
    /// without wiping, into setup, which finishes the wipe with nothing
    /// running.
    pub stop: Vec<String>,
    /// Run after the wipe when setup isn't started in place (#4500): a
    /// keeper's plan, a lamp without setup's unit, a start that fails, or a
    /// stop or wipe that fails.
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
    /// What a command that only reads printed: the parts of the lamp's
    /// target and whether they stopped ([`stop_and_wait`]).
    fn read(&mut self, argv: &[String]) -> io::Result<String>;
    fn sync(&mut self);
    /// The last resort when the reboot command fails: a lamp left dark with
    /// no marker must still come back, into setup.
    fn force_reboot(&mut self);
    /// Before setup is started in place (#4500): what a reboot cleared by
    /// being memory, [`VOLATILE`], emptied, each folder kept.
    fn clear_volatile(&mut self) -> io::Result<()>;
    /// One of [`FORGET`]'s commands. A lamp's runs bounded, as setup's other
    /// `wpa_cli` and `journalctl` calls are ([`System`]'s, `QUERY_TIMEOUT`);
    /// by default as [`Effects::run`].
    fn forget(&mut self, argv: &[String]) -> io::Result<()> {
        self.run(argv)
    }
    /// The skills a person put on from a package removed, [`remove_installed_skills`]
    /// in [`SKILLS`]; the folders removed.
    fn remove_installed_skills(&mut self) -> io::Result<Vec<PathBuf>>;
}

/// The product's skills folder (`lamp_update::product_skills::FOLDER`).
pub const SKILLS: &str = lamp_update::product_skills::FOLDER;

/// Each skill in `folder` a person put on from a package, made by voice
/// (`LAMP_MAKE_SKILL`) or from the Store, removed with its folder: it holds
/// `lamp_skills::package::INSTALLED_FROM`, which a skill that comes with
/// the lamp never does, so the lamp's own stay, as install and each round
/// put them back. A made skill is the person's, and it outlived the plan's
/// wipes (the Queue, #4710, 1 Nov 02:4x). Not a plan path: the plan empties
/// a path and keeps it, and a plan field this build doesn't know refuses
/// the whole reset. A link is passed over and a missing folder removes
/// nothing. Returns the folders removed, in order.
pub fn remove_installed_skills(folder: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut skills = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let skill = entry.path();
        match fs::symlink_metadata(skill.join(lamp_skills::package::INSTALLED_FROM)) {
            Ok(_) => skills.push(skill),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    skills.sort();
    for skill in &skills {
        wipe(skill)?;
        fs::remove_dir(skill)?;
    }
    if !skills.is_empty() {
        File::open(folder)?.sync_all()?;
    }
    Ok(skills)
}

/// What a part keeps in `/run` past its own stop: the tmpfiles' folders (the
/// bus's latest messages, which can say a person's name or words, and the
/// app's photos for the eyes) and each `RuntimeDirectory=` its unit keeps
/// (`RuntimeDirectoryPreserve=yes`: the voice's, with the thread's set-aside
/// words and the room, and the records'). Every other `RuntimeDirectory=` is
/// removed by systemd as its unit stops. A reboot empties `/run`; a reset
/// that starts setup in place empties these, with every part stopped.
pub const VOLATILE: [&str; 4] = [
    "/run/lampos-bus",
    "/run/lampos-photos",
    "/run/lampos-voice",
    "/run/lamp-records",
];

pub struct System;

impl Effects for System {
    fn run(&mut self, argv: &[String]) -> io::Result<()> {
        let status = Command::new(&argv[0]).args(&argv[1..]).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("{} ended with {status}", argv[0])))
        }
    }
    fn read(&mut self, argv: &[String]) -> io::Result<String> {
        query::read(argv)
    }
    fn sync(&mut self) {
        rustix::fs::sync();
    }
    fn force_reboot(&mut self) {
        rustix::fs::sync();
        #[cfg(target_os = "linux")]
        let _ = rustix::system::reboot(rustix::system::RebootCommand::Restart);
    }
    fn clear_volatile(&mut self) -> io::Result<()> {
        for folder in VOLATILE {
            wipe(Path::new(folder))?;
        }
        Ok(())
    }
    fn forget(&mut self, argv: &[String]) -> io::Result<()> {
        run_within(argv, crate::wifi::QUERY_TIMEOUT)
    }
    fn remove_installed_skills(&mut self) -> io::Result<Vec<PathBuf>> {
        remove_installed_skills(Path::new(SKILLS))
    }
}

/// `argv` run and killed with its children once `most` has passed
/// ([`crate::wifi::within`]): a stuck journald or wpa_supplicant never holds
/// back setup's start in place (the Reviewer-lead's P3 on #4533).
pub fn run_within(argv: &[String], most: std::time::Duration) -> io::Result<()> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    let (status, _) = crate::wifi::within(&mut command, most)?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{} ended with {status}", argv[0])))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "step", content = "path")]
pub enum Step {
    Journal,
    Stopped,
    Wiped(PathBuf),
    MarkerRemoved,
    JournalRemoved,
    /// Setup and what a boot starts, started in place of a reboot (#4500).
    Started,
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

/// Run the plan's stop and return once the lamp has stopped. A `systemctl
/// stop` of a target is run with every part of the target named too, so it
/// returns only once each part has stopped. `systemctl stop lampos.target`
/// alone returns as soon as the target's own job is done, before its parts
/// have stopped (systemd 252 on lamp-4ace on 26 Oct: 0.04 s, and a part's
/// write on stop 3.1 s later; it waits only for the job it named, Third on
/// #1711). So a part that writes as it stops wrote back into what the reset
/// had just emptied: on lamp-29e7 that day, Voice's onboarding commit kept
/// the factory session's steps through the reset, and the customer got no
/// first five minutes. Named, each part's stop job is waited for. Then each
/// part must read stopped (`inactive`, or `failed` once ended by force), and
/// the parts read again must be the same, none joined during the stop
/// (Third's checks): otherwise it is a stop that failed, and nothing is
/// wiped. A stop of `lampos.target` also stops the product's writers of
/// wiped paths that are not part of it ([`WRITERS`]), what starts them
/// first ([`STARTERS`]), so that none starts after they are listed, and
/// checks them the same way. A stop that is not `systemctl stop`
/// (`lamp-keeper stop lamp`) is the plan's as it is.
pub fn stop_and_wait(stop: &[String], effects: &mut impl Effects) -> io::Result<()> {
    stop_and_wait_keeping(stop, effects).map(|_| ())
}

/// The lamp's setup, which serves when the marker is gone and brings up the
/// hotspot from a station live, as Wi-Fi's recovery does.
pub const SETUP_UNIT: &str = "lamp-setup.service";

/// [`stop_and_wait`], and what to start again in place of a reboot after a
/// product reset's wipe (#4500, the A733 board's warm reset hangs one time
/// in three): the [`STARTERS`] and `lamp-update.service` that were active
/// before the stop, which a boot starts outside `lampos.target`, and
/// [`SETUP_UNIT`]. None (empty) unless the stop is `systemctl stop` of
/// `lampos.target` and setup's unit is loaded: then the lamp reboots, as
/// before. The updater's way back is not named: its timer starts it.
pub fn stop_and_wait_keeping(
    stop: &[String],
    effects: &mut impl Effects,
) -> io::Result<Vec<String>> {
    let systemctl = stop
        .first()
        .is_some_and(|program| Path::new(program).file_name() == Some("systemctl".as_ref()));
    if !systemctl || stop.get(1).map(String::as_str) != Some("stop") {
        return effects.run(stop).map(|()| Vec::new());
    }
    let product = stop[2..].iter().any(|unit| unit == "lampos.target");
    // Only loaded units are named: a stop that names a unit systemd does
    // not have fails (systemd 252), and one it does not have is not running.
    let listed_starters = if product {
        listed(&stop[0], &STARTERS, effects)?
    } else {
        Vec::new()
    };
    let mut again: Vec<String> = listed_starters
        .iter()
        .filter(|(unit, active)| *active && unit != "lamp-update-back.service")
        .map(|(unit, _)| unit.clone())
        .collect();
    let starters: Vec<String> = listed_starters.into_iter().map(|(unit, _)| unit).collect();
    if !starters.is_empty() {
        let mut argv = vec![stop[0].clone(), "stop".into()];
        argv.extend(starters.iter().cloned());
        effects.run(&argv)?;
    }
    let writers = |effects: &mut _| -> io::Result<(Vec<String>, Listed)> {
        let mut parts = parts_of(stop, effects)?;
        let found = if product {
            listed(&stop[0], &WRITERS, effects)?
        } else {
            Vec::new()
        };
        for (writer, _) in &found {
            if !stop[2..].contains(writer) && !parts.contains(writer) {
                parts.push(writer.clone());
            }
        }
        Ok((parts, found))
    };
    let (parts, listed_writers) = writers(effects)?;
    // The updater was running, and setup can be started: else no start in
    // place, and the lamp reboots.
    if listed_writers
        .iter()
        .any(|(unit, active)| *active && unit == "lamp-update.service")
    {
        again.push("lamp-update.service".into());
    }
    if listed_writers.iter().any(|(unit, _)| unit == SETUP_UNIT) {
        again.push(SETUP_UNIT.into());
    } else {
        again.clear();
    }
    let mut argv = stop.to_vec();
    argv.extend(parts.iter().cloned());
    effects.run(&argv)?;
    let parts: Vec<String> = starters.into_iter().chain(parts).collect();
    if parts.is_empty() {
        return Ok(again);
    }
    let mut show = vec![
        stop[0].clone(),
        "show".into(),
        "-p".into(),
        "ActiveState".into(),
        "--value".into(),
    ];
    show.extend(parts.iter().cloned());
    let states = effects.read(&show)?;
    let states: Vec<&str> = states
        .lines()
        .map(str::trim)
        .filter(|state| !state.is_empty())
        .collect();
    if states.len() != parts.len()
        || states
            .iter()
            .any(|state| !matches!(*state, "inactive" | "failed"))
    {
        return Err(io::Error::other(format!(
            "not every part of the lamp stopped: {}",
            parts
                .iter()
                .zip(states.iter().chain(std::iter::repeat(&"unread")))
                .map(|(part, state)| format!("{part} {state}"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if let Some(joined) = writers(effects)?
        .0
        .into_iter()
        .find(|part| !parts.contains(part))
    {
        return Err(io::Error::other(format!(
            "{joined} became part of the lamp during its stop"
        )));
    }
    Ok(again)
}

/// What starts a writer of wiped paths outside `lampos.target` on a product
/// lamp: stopped before the writers are listed, so none starts after
/// (Second on #3579). `lamp-skills-manage.socket` is the sockets target's,
/// and `lamp-records.timer` the timers'. The updater's way back, its timer
/// and a worker already running, can restart `lamp-update.service` for a
/// pending self-switch (Reviewer-two's P2 on #3579), and the keeper's
/// restart socket restarts parts: each is stopped here, before the barrier,
/// so nothing restarts a writer after it.
pub const STARTERS: [&str; 8] = [
    "lamp-skills-manage.socket",
    "lamp-setup-change.socket",
    "lamp-setup-keys.socket",
    "lamp-setup-wifi.socket",
    "lamp-records.timer",
    "lamp-update-back.timer",
    "lamp-update-back.service",
    "lamp-keeper-restart.socket",
];

/// Writers of paths the product plan wipes that are not part of
/// `lampos.target` (reviewer-three's P2 on #3579, the reviewer-lead's and
/// Second's): setup serving for 600 s after setup (the setup files and the
/// credential), its recovery, the change, keys and Wi-Fi helpers' accepted
/// instances (the setup files, `/etc/lampOS/voice/own`, the Wi-Fi), the
/// skills' manager's instances (`/var/lib/lampos-skills-state`), the
/// records' check (`/var/lib/lampOS/voice/sessions`), and the updater and
/// its apply (the skills' state, `/var/lib/lampOS`; the reboot after the
/// reset would end them all the same), and the boot's Wi-Fi restore (the
/// Wi-Fi file; Third on #3579). And `lamp-peek`, an office lamp's page,
/// which a reboot kept off by its setup condition: a start in place would
/// leave it holding port 80 from setup's page (#4536); stopped, it is not
/// started again until the lamp next boots. Patterns name only loaded
/// units (no product lamp has `lamp-peek`), and none matches
/// `lamp-setup-wipe.service`, the reset's own unit.
pub const WRITERS: [&str; 11] = [
    "lamp-setup.service",
    "lamp-setup-recover.service",
    "lamp-setup-change@*.service",
    "lamp-setup-keys@*.service",
    "lamp-setup-wifi@*.service",
    "lamp-skills-manage@*.service",
    "lamp-records.service",
    "lamp-update.service",
    "lamp-update-apply.service",
    "lamp-setup-wifi-restore.service",
    "lamp-peek.service",
];

/// Units as [`listed`] gives them: each name, and whether it is active.
type Listed = Vec<(String, bool)>;

/// The loaded units among `units` (names or patterns), as `systemctl
/// list-units --all` lists them: the first word of each line whose LOAD
/// column says `loaded`, never a `not-found` or masked stub, which a stop
/// would refuse (the reviewer-lead's P3 on #3579). Each with whether its
/// ACTIVE column says `active`, for what a reset starts again (#4500).
fn listed(systemctl: &str, units: &[&str], effects: &mut impl Effects) -> io::Result<Listed> {
    let mut argv: Vec<String> = [
        systemctl,
        "list-units",
        "--all",
        "--plain",
        "--no-legend",
        "--full",
    ]
    .map(str::to_owned)
    .into();
    argv.extend(units.iter().map(|unit| (*unit).to_owned()));
    Ok(effects
        .read(&argv)?
        .lines()
        .filter_map(|line| {
            let mut words = line
                .split_whitespace()
                .skip_while(|word| *word == "\u{25cf}");
            let unit = words.next()?;
            (words.next() == Some("loaded") && !unit.starts_with('-'))
                .then(|| (unit.to_owned(), words.next() == Some("active")))
        })
        .collect())
}

/// The units that are part of each target a `systemctl stop` names:
/// systemd's `ConsistsOf` of it, every unit `PartOf` it, and theirs for a
/// part that is a target itself. A printed word that reads as an option is
/// never taken, and a unit already named is not named again.
fn parts_of(stop: &[String], effects: &mut impl Effects) -> io::Result<Vec<String>> {
    // More targets than any lamp has, nested: a loop in what systemd prints
    // cannot keep the reset reading.
    const MOST_TARGETS: usize = 64;
    let mut parts: Vec<String> = Vec::new();
    let mut targets: Vec<String> = stop[2..]
        .iter()
        .filter(|unit| unit.ends_with(".target"))
        .cloned()
        .collect();
    let mut read = 0;
    while let Some(target) = targets.pop() {
        read += 1;
        if read > MOST_TARGETS {
            return Err(io::Error::other(format!(
                "more than {MOST_TARGETS} targets in the lamp's stop"
            )));
        }
        let show = [
            stop[0].clone(),
            "show".into(),
            "-p".into(),
            "ConsistsOf".into(),
            "--value".into(),
            target,
        ];
        for part in effects.read(&show)?.split_whitespace() {
            if part.starts_with('-') || stop[2..].iter().chain(&parts).any(|unit| unit == part) {
                continue;
            }
            if part.ends_with(".target") {
                targets.push(part.to_owned());
            }
            parts.push(part.to_owned());
        }
    }
    Ok(parts)
}

/// Begin a reset: journal, remove the marker, stop the lamp, wipe, then start
/// setup in place ([`stop_and_wait_keeping`]), or reboot where it can't.
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
    // Parts that cannot be read, or not all stopped, are as a stop that
    // fails: nothing is wiped while one may still be running.
    let again = match stop_and_wait_keeping(&plan.stop, effects) {
        Ok(again) => again,
        Err(error) => {
            eprintln!(
                "lamp-setup: reset: the lamp did not stop ({error}); rebooting into setup, which finishes the wipe at its start"
            );
            reboot(plan, effects, &mut steps);
            return Ok(steps);
        }
    };
    steps.push(Step::Stopped);
    if let Err(error) = finish(plan, effects, &mut steps) {
        // The journal and the missing marker bring the lamp back into setup,
        // which finishes the wipe; a lamp left dark here would wait for a hand.
        reboot(plan, effects, &mut steps);
        return Err(error);
    }
    if again.is_empty() {
        reboot(plan, effects, &mut steps);
    } else {
        start_in_place(plan, &again, effects, &mut steps);
    }
    Ok(steps)
}

/// Run before setup is started in place, after [`VOLATILE`] is emptied, each
/// best effort (Reviewer-three's, Fourth's and Reviewer-two's P3s on #4513):
/// wpa_supplicant leaves the old owner's network, which it still holds in
/// memory with its file wiped, so the lamp is off it before setup's hotspot
/// takes the radio; then the journal, which a reboot emptied (it is a RAM
/// disk on the product) and which names the networks joined, is rotated and
/// emptied. The start's own line is written after it. Absolute, as the
/// plan's own commands are (Reviewer-two's P3 on #4513): Debian 12's
/// merged `/usr`, so a unit's `PATH` is never asked. Each is bounded, as
/// setup's other calls of them are ([`Effects::forget`]).
pub const FORGET: [&[&str]; 4] = [
    &["/usr/sbin/wpa_cli", "-i", "wlan0", "disconnect"],
    &["/usr/sbin/wpa_cli", "-i", "wlan0", "remove_network", "all"],
    &["/usr/bin/journalctl", "--rotate"],
    &["/usr/bin/journalctl", "--vacuum-time=1s"],
];

/// The lamp back into setup without a reboot (#4500): what the reset stopped
/// that a boot starts, and setup, which serves with the marker gone and
/// brings up its hotspot. Every part of `lampos.target` waits for the
/// marker, which setup writes again. A start that fails reboots, as before,
/// so a reset never leaves the lamp dark by itself.
fn start_in_place(
    plan: &Plan,
    again: &[String],
    effects: &mut impl Effects,
    steps: &mut Vec<Step>,
) {
    // What a reboot would have emptied, emptied first: the last person's
    // latest messages on the bus and photos never reach the next setup.
    if let Err(error) = effects.clear_volatile() {
        eprintln!("lamp-setup: reset: /run could not be emptied ({error}); rebooting");
        reboot(plan, effects, steps);
        return;
    }
    // What else a reboot dropped from memory, best effort: the old network
    // wpa_supplicant still runs, though its file is wiped, and the journal,
    // which keeps the networks joined (a RAM disk on the product). Neither
    // keeps the start from going on. What failed is said once, after the
    // vacuum, which would take a line written before it (Reviewer-three's
    // P3 on #4533).
    let mut failed = Vec::new();
    for forget in FORGET {
        let argv: Vec<String> = forget.iter().map(|word| (*word).to_owned()).collect();
        if let Err(error) = effects.forget(&argv) {
            failed.push(format!("{} ({error})", argv.join(" ")));
        }
    }
    if !failed.is_empty() {
        eprintln!(
            "lamp-setup: reset: not forgotten, going on: {}",
            failed.join("; ")
        );
    }
    let mut argv = vec![plan.stop[0].clone(), "start".into()];
    argv.extend(again.iter().cloned());
    match effects.run(&argv) {
        Ok(()) => {
            eprintln!(
                "lamp-setup: reset: started in place, no reboot: {}",
                again.join(" ")
            );
            steps.push(Step::Started);
        }
        Err(error) => {
            eprintln!(
                "lamp-setup: reset: setup could not be started in place ({error}); rebooting"
            );
            reboot(plan, effects, steps);
        }
    }
}

fn reboot(plan: &Plan, effects: &mut impl Effects, steps: &mut Vec<Step>) {
    steps.push(Step::Rebooting);
    if effects.run(&plan.reboot).is_err() {
        effects.force_reboot();
    }
}

/// At start, before anything else: finish a reset a power cut interrupted.
/// Nothing of the lamp is running yet, so nothing is stopped and there is no
/// second reboot. Returns the steps taken, empty when there was no reset.
pub fn resume(plan: &Plan, effects: &mut impl Effects) -> io::Result<Vec<Step>> {
    plan.check().map_err(io::Error::other)?;
    let mut steps = Vec::new();
    match fs::symlink_metadata(&plan.journal) {
        Ok(_) => finish(plan, effects, &mut steps)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(steps)
}

fn finish(plan: &Plan, effects: &mut impl Effects, steps: &mut Vec<Step>) -> io::Result<()> {
    for path in &plan.wipe {
        if path.file_name() == Some(WIFI_FILE.as_ref()) {
            for copy in WIFI_COPIES {
                wipe(&path.with_extension(copy))?;
            }
        }
        // Setup's staging copy of a file it writes (`serve::stage`), left
        // beside it by a write cut mid-way (the reviewer-lead's P3 on #3579).
        wipe(&path.with_extension(STAGED))?;
        wipe(path)?;
        steps.push(Step::Wiped(path.clone()));
    }
    for skill in effects.remove_installed_skills()? {
        steps.push(Step::Wiped(skill));
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
