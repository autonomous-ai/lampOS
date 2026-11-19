//! One remove, in one order, every step safe to run again. A remove is cut by
//! a crash, by the helper's own time limit or by a failed step, and no undo
//! runs then: the `removing` mark is written first and cleared last, and the
//! start-up sweep ([`crate::sweep`]) runs this again for every app that
//! still has it.
//!
//! Order: mark; stop both unit variants; wait until neither has a process;
//! clear their failed state; `systemctl clean --what=state` for both (systemd removes the dynamic user's data and its link; the helper writes in no state folder) and a look that it is gone; lamp-control's `app_forget` (what it kept of the app); the code;
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

/// systemd's own state removal has run (`systemctl clean --what=state`):
/// the app's private data folder and its link are gone, or the remove stops
/// here with the `removing` mark still set. Nothing is deleted by the helper.
fn data_gone(layout: &Layout, name: &str) -> Result<(), String> {
    for path in [layout.data_link(name), layout.data_real(name)] {
        if fs::symlink_metadata(&path).is_ok() {
            return Err(format!(
                "{} is still there after systemctl clean: left alone",
                path.display()
            ));
        }
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
        || there(&layout.data_link(name))
        || there(&layout.data_real(name))
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
    // The data is systemd's: it made it, so it removes it.
    for unit in &units {
        fx.systemctl(Verb::Clean, unit)?;
    }
    data_gone(layout, name)?;
    // Both variants have stopped: lamp-control drops what it kept of the app
    // (settings, panel, notes, clips, moves) and closes its connections. An
    // answer that is not a yes leaves the remove cut at its mark, resumed by
    // the next sweep or the owner asking again; an app left half-forgotten
    // would hand its notes to a new app of the same name.
    fx.forget(name)?;
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
