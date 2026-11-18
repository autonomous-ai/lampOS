//! What the helper does first at every start: finishes what a crash, the
//! helper's own time limit or a power cut cut short, and clears what is
//! stale. A step that fails is reported and the rest still run.
use crate::{
    effects::Effects,
    layout::Layout,
    remove::{Companions, remove, remove_entry},
    state::{Mark, clear, marked},
};
use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub resumed_removes: Vec<String>,
    pub cleaned_installs: Vec<String>,
    pub errors: Vec<String>,
}

/// An install or update cut short: temporary folders go; with no version
/// linked as `current` the whole code folder and the companion skills placed
/// before it go; with one linked, every other version goes.
fn clean_install(layout: &Layout, skills: &mut dyn Companions, name: &str) -> Result<(), String> {
    let app = layout.code.join(name);
    let mut keep: Option<String> = None;
    if let Ok(target) = fs::read_link(app.join("current"))
        && let Some(version) = target.file_name().and_then(|v| v.to_str())
        && fs::symlink_metadata(app.join(version)).is_ok_and(|m| m.is_dir())
    {
        keep = Some(version.to_owned());
    }
    match keep {
        None => {
            remove_entry(&app).map_err(|e| format!("{}: {e}", app.display()))?;
            skills.remove_for(name)?;
        }
        Some(version) => {
            let entries = fs::read_dir(&app).map_err(|e| format!("{}: {e}", app.display()))?;
            for entry in entries.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                if file != "current" && file != version {
                    remove_entry(&entry.path())
                        .map_err(|e| format!("{}: {e}", entry.path().display()))?;
                }
            }
        }
    }
    clear(&layout.state, name, Mark::Installing).map_err(|e| format!("a mark: {e}"))
}

pub fn sweep(layout: &Layout, fx: &mut dyn Effects, skills: &mut dyn Companions) -> Report {
    let mut report = Report::default();
    for (name, marks) in marked(&layout.state) {
        if marks.contains(&Mark::Removing) {
            match remove(layout, fx, skills, &name) {
                Ok(_) => report.resumed_removes.push(name),
                Err(error) => report.errors.push(format!("{name}: {error}")),
            }
            continue;
        }
        if marks.contains(&Mark::Installing) {
            match clean_install(layout, skills, &name) {
                Ok(()) => report.cleaned_installs.push(name.clone()),
                Err(error) => report.errors.push(format!("{name}: {error}")),
            }
        }
        // A stopped mark with no app behind it (a remove that failed late,
        // an install that never finished) must not make a later install of
        // the same name start stopped.
        if marks.contains(&Mark::Stopped) && fs::symlink_metadata(layout.code.join(&name)).is_err()
        {
            let _ = clear(&layout.state, &name, Mark::Stopped);
        }
    }
    report
}

/// Staged files too old, or too many: links and files go (a link is
/// unlinked, never followed), folders are left. Returns how many went.
pub fn sweep_staging(
    staging: &Path,
    now: SystemTime,
    max_age: Duration,
    max_count: usize,
) -> usize {
    let mut removed = 0;
    let mut kept: Vec<(SystemTime, std::path::PathBuf)> = Vec::new();
    let Ok(entries) = fs::read_dir(staging) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            continue;
        }
        let stamp = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let old = now.duration_since(stamp).unwrap_or_default() > max_age;
        if meta.file_type().is_symlink() || old {
            if fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        } else {
            kept.push((stamp, path));
        }
    }
    kept.sort();
    let over = kept.len().saturating_sub(max_count);
    for (_, path) in kept.into_iter().take(over) {
        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}
