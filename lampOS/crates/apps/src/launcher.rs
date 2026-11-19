//! The fixed launcher the app units run (`python3 -I <code>/.run.py`) and the
//! net units' resolver (`.resolv.conf`): the helper writes each, root's and
//! world-readable, from the copy built into it ([`crate::LAUNCHER`],
//! [`crate::RESOLV`]), whenever the file there differs. One rename, so a
//! unit never reads half of it. Its name starts with a dot: it is no app.
use crate::{LAUNCHER, RESOLV, layout::Layout};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt};

/// Puts the launcher and the net units' resolver in place, each if it is not
/// exactly this build's.
pub fn ensure(layout: &Layout) -> Result<(), String> {
    put(layout, &layout.launcher(), ".run.py.new", LAUNCHER)?;
    put(layout, &layout.resolver(), ".resolv.conf.new", RESOLV)
}

fn put(layout: &Layout, path: &std::path::Path, fresh: &str, text: &str) -> Result<(), String> {
    if fs::read(path).is_ok_and(|have| have == text.as_bytes()) {
        return Ok(());
    }
    fs::create_dir_all(&layout.code).map_err(|e| format!("{}: {e}", layout.code.display()))?;
    let fresh = layout.code.join(fresh);
    match fs::remove_file(&fresh) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("{}: {e}", fresh.display()));
        }
        _ => {}
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&fresh)
        .map_err(|e| format!("{}: {e}", fresh.display()))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", fresh.display()))?;
    fs::rename(&fresh, path).map_err(|e| format!("{}: {e}", path.display()))
}
