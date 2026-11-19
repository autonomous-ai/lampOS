//! One remove, in one order, every step safe to run again. A remove is cut by
//! a crash, by the helper's own time limit or by a failed step, and no undo
//! runs then: the `removing` mark is written first and cleared last, and the
//! start-up sweep ([`crate::sweep`]) runs this again for every app that
//! still has it.
//!
//! Order: mark; stop both unit variants; wait until neither has a process;
//! clear their failed state; the data (never through the link); the code;
//! the companion skills (with the apps lock let go); the stopped mark; the
//! removing mark. There is no
//! user to delete: systemd's dynamic user goes with the unit.
use crate::{
    effects::{Effects, Verb},
    layout::Layout,
    lock::AppsLock,
    name::{app_name_ok, net_unit, plain_unit},
    package::SkillFolder,
    state::{Mark, clear, has, set},
};
use std::{fs, io, path::Path};

/// What a remove of an app that is not there says; nothing was done.
pub const NOT_INSTALLED: &str = "not installed";
/// How many times a unit is asked whether its last process is gone.
pub const WAIT_POLLS: u32 = 40;

#[derive(Debug, PartialEq, Eq)]
pub enum Removed {
    Done,
}

/// The app's companion skills, placed and removed by the helper itself
/// (under the skills lock, never held with the apps' own).
///
/// Neither call is ever made with the apps lock held: the skills have a lock
/// of their own and the two are never nested.
pub trait Companions {
    /// Removes every skill of the app's; none is not an error.
    fn remove_for(&mut self, app: &str) -> Result<(), String>;
    /// Puts the app's companion skills in, replacing the ones it had, with
    /// the skill checks and under the skills lock; all or none.
    fn place(&mut self, app: &str, folders: &[SkillFolder]) -> Result<(), String>;
    /// Whether a skill (core, installed or made) already has this name.
    fn taken_by_skill(&mut self, name: &str) -> bool;
}

fn gone_ok(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn wait_empty(fx: &mut dyn Effects, unit: &str) -> Result<(), String> {
    for _ in 0..WAIT_POLLS {
        if fx.unit_empty(unit)? {
            return Ok(());
        }
        fx.pause();
    }
    Err(format!("{unit} still has a process"))
}

/// Removes one entry without following it: a link is unlinked, a folder
/// emptied by `remove_dir_all` (which never follows a link, and unlinks a
/// hard link without touching its other names).
pub(crate) fn remove_entry(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// The data folder is at `private/<name>` behind the link `data/<name>`
/// that systemd makes. The link must be exactly that or it is left alone,
/// with whatever it points at.
fn remove_data(layout: &Layout, name: &str) -> Result<(), String> {
    let link = layout.data.join(name);
    let real = layout.private.join(name);
    let is_link = match fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let target = fs::read_link(&link).map_err(|e| format!("{}: {e}", link.display()))?;
            if target != real {
                return Err(format!(
                    "the data link {} points elsewhere ({}): left alone",
                    link.display(),
                    target.display()
                ));
            }
            true
        }
        Ok(_) => {
            return Err(format!(
                "{} is not a link to the data folder: left alone",
                link.display()
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("{}: {error}", link.display())),
    };
    remove_entry(&real).map_err(|e| format!("{}: {e}", real.display()))?;
    if is_link {
        gone_ok(fs::remove_file(&link)).map_err(|e| format!("{}: {e}", link.display()))?;
    }
    Ok(())
}

pub fn remove(
    layout: &Layout,
    fx: &mut dyn Effects,
    skills: &mut dyn Companions,
    name: &str,
) -> Result<Removed, String> {
    if !app_name_ok(name) {
        return Err(format!("not an app name: {name:?}"));
    }
    let held = AppsLock::take(&layout.state, layout.lock_wait)?;
    let there = |path: &Path| fs::symlink_metadata(path).is_ok();
    if !(there(&layout.code.join(name))
        || there(&layout.data.join(name))
        || there(&layout.private.join(name))
        || has(&layout.state, name, Mark::Removing))
    {
        return Err(NOT_INSTALLED.to_owned());
    }
    set(&layout.state, name, Mark::Removing).map_err(|e| format!("the removing mark: {e}"))?;
    // Both variants, always: which one ran last is not something to trust.
    let units = [plain_unit(name), net_unit(name)];
    for unit in &units {
        fx.systemctl(Verb::Stop, unit)?;
    }
    for unit in &units {
        wait_empty(fx, unit)?;
    }
    for unit in &units {
        fx.systemctl(Verb::ResetFailed, unit)?;
    }
    remove_data(layout, name)?;
    let code = layout.code.join(name);
    remove_entry(&code).map_err(|e| format!("{}: {e}", code.display()))?;
    // The skills have their own lock: not taken inside this one.
    drop(held);
    skills.remove_for(name)?;
    let _held = AppsLock::take(&layout.state, layout.lock_wait)?;
    for mark in [Mark::Stopped, Mark::Installing, Mark::Removing] {
        clear(&layout.state, name, mark).map_err(|e| format!("a mark: {e}"))?;
    }
    Ok(Removed::Done)
}
