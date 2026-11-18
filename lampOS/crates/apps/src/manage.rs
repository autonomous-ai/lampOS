//! Start, stop, start-all, list, and the owner's remove: what the helper
//! does to an app that is already installed. Every verb runs under the apps
//! lock, and none runs on an app that is being installed or removed.
use crate::{
    effects::{Effects, Verb},
    install::{Record, installed_record},
    layout::Layout,
    lock::AppsLock,
    name::{app_name_ok, net_unit, plain_unit},
    remove::{Companions, NOT_INSTALLED, Removed, remove},
    state::{Mark, clear, has, set},
    yes::{Op, Yes, YesKey},
};
use std::fs;

pub use crate::install::BUSY_APP;

#[derive(Debug, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub version: String,
    pub grants: Vec<String>,
    pub net: bool,
    /// The owner stopped it.
    pub stopped: bool,
}

fn unit_for(record: &Record, name: &str) -> String {
    if record.net {
        net_unit(name)
    } else {
        plain_unit(name)
    }
}

/// The installed record of an app that may be started or stopped now.
fn settled(layout: &Layout, name: &str) -> Result<Record, String> {
    if !app_name_ok(name) {
        return Err(format!("not an app name: {name:?}"));
    }
    if has(&layout.state, name, Mark::Removing) || has(&layout.state, name, Mark::Installing) {
        return Err(BUSY_APP.to_owned());
    }
    installed_record(layout, name).ok_or_else(|| NOT_INSTALLED.to_owned())
}

/// Starts the app (the variant it was installed as) and forgets the owner's
/// stop. The other variant is stopped first: never two at once.
pub fn start(layout: &Layout, fx: &mut dyn Effects, name: &str) -> Result<(), String> {
    let _held = AppsLock::take(&layout.state, layout.lock_wait)?;
    let record = settled(layout, name)?;
    let other = if record.net {
        plain_unit(name)
    } else {
        net_unit(name)
    };
    fx.systemctl(Verb::Stop, &other)?;
    fx.systemctl(Verb::Start, &unit_for(&record, name))?;
    clear(&layout.state, name, Mark::Stopped).map_err(|e| format!("a mark: {e}"))
}

/// Stops both variants and marks the app stopped, so an update and the boot
/// start leave it so. The mark is written first: a cut stop is still a stop.
pub fn stop(layout: &Layout, fx: &mut dyn Effects, name: &str) -> Result<(), String> {
    let _held = AppsLock::take(&layout.state, layout.lock_wait)?;
    settled(layout, name)?;
    set(&layout.state, name, Mark::Stopped).map_err(|e| format!("a mark: {e}"))?;
    for unit in [plain_unit(name), net_unit(name)] {
        fx.systemctl(Verb::Stop, &unit)?;
    }
    Ok(())
}

fn names(layout: &Layout) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&layout.code)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| app_name_ok(n))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The boot start: every installed app that is not stopped, not being
/// changed, and has a readable record. One that fails to start is reported
/// and the rest still start. Returns the names started and the errors.
pub fn start_all(layout: &Layout, fx: &mut dyn Effects) -> (Vec<String>, Vec<String>) {
    let (mut started, mut errors) = (Vec::new(), Vec::new());
    for name in names(layout) {
        let Ok(_held) = AppsLock::take(&layout.state, layout.lock_wait) else {
            errors.push(format!("{name}: {}", crate::lock::BUSY));
            continue;
        };
        if has(&layout.state, &name, Mark::Stopped) {
            continue;
        }
        let Ok(record) = settled(layout, &name) else {
            continue;
        };
        match fx.systemctl(Verb::Start, &unit_for(&record, &name)) {
            Ok(()) => started.push(name),
            Err(error) => errors.push(format!("{name}: {error}")),
        }
    }
    (started, errors)
}

/// The installed apps, by name. Read-only: it takes no lock.
pub fn list(layout: &Layout) -> Vec<Listed> {
    names(layout)
        .into_iter()
        .filter(|n| {
            !has(&layout.state, n, Mark::Removing) && !has(&layout.state, n, Mark::Installing)
        })
        .filter_map(|name| {
            let record = installed_record(layout, &name)?;
            Some(Listed {
                stopped: has(&layout.state, &name, Mark::Stopped),
                name,
                version: record.version,
                grants: record.grants,
                net: record.net,
            })
        })
        .collect()
}

/// The owner's remove: the yes is for the installed version and package
/// (no grants), used up before anything is touched; then the remove runs.
/// An app that is not installed asks no yes.
pub fn remove_with_yes(
    layout: &Layout,
    fx: &mut dyn Effects,
    skills: &mut dyn Companions,
    yes: &mut dyn Yes,
    name: &str,
) -> Result<Removed, String> {
    if !app_name_ok(name) {
        return Err(format!("not an app name: {name:?}"));
    }
    {
        let _held = AppsLock::take(&layout.state, layout.lock_wait)?;
        // A remove already begun is resumed without a second yes.
        if !has(&layout.state, name, Mark::Removing) {
            if has(&layout.state, name, Mark::Installing) {
                return Err(BUSY_APP.to_owned());
            }
            let record = installed_record(layout, name).ok_or_else(|| NOT_INSTALLED.to_owned())?;
            yes.take(&YesKey {
                op: Op::Remove,
                name: name.to_owned(),
                version: record.version,
                sha256: record.sha256,
                grants: Vec::new(),
                net: false,
                logs: false,
            })
            .map_err(|why| format!("no yes: {why}"))?;
            set(&layout.state, name, Mark::Removing).map_err(|e| format!("a mark: {e}"))?;
        }
    }
    remove(layout, fx, skills, name)
}
