//! The fixed one-shot unit that refreshes the settings writer (#3689,
//! docs/updates.md, "The writer's one-shot unit").
//!
//! **Unwired:** no round calls [`run`] yet, and `writer-refresh` is not a
//! command yet. A round's sandbox may write `/etc/systemd/system` but not
//! `/usr/local/lib/lampos`, where the fixed writer is. So a round writes
//! [`TEXT`] as [`UNIT`] and starts it. systemd then runs the confirmed
//! lamp-update as root under this unit's own sandbox, which may write only
//! that folder and the records.
//!
//! Before it starts the unit, the round checks that what systemd loaded is
//! this file and only this file: loaded from `/etc/systemd/system`, with no
//! drop-in, not masked. Any other unit at the name is refused, never
//! written over through a link.
use crate::{
    records::write_whole,
    world::{Systemd, clean_line},
};
use std::{collections::BTreeMap, fs, io, path::Path};

/// The unit's name. Fixed: a round passes no other name to systemd here.
pub const UNIT: &str = "lamp-update-writer.service";
/// Where a round writes it, under the lamp's root.
pub const FOLDER: &str = "etc/systemd/system";
/// The properties [`loaded_as_written`] reads, in `systemctl show`'s form.
pub const PROPERTIES: &str = "Id,LoadState,FragmentPath,DropInPaths,UnitFileState";

/// The unit. It starts the confirmed lamp-update through its `current`
/// link: a round starts it only once no switch of lamp-update's own is
/// waiting (`self.json` gone), and the command checks that again itself.
pub const TEXT: &str = "\
# lamp-update-writer: puts a checked copy of the confirmed lamp-update at the settings writer's fixed path
# (lampOS/docs/updates.md, \"The writer's one-shot unit\", #3689). Written and started by a round of lamp-update,
# whose own sandbox cannot write /usr/local/lib/lampos; never enabled. Outside lampos.target, as lamp-update is.
[Unit]
Description=lampOS updates, the settings writer refreshed (lamp-update-writer)

[Service]
Type=oneshot
User=root
Group=root
ExecStart=/opt/lampOS/update/current/lamp-update writer-refresh
StandardInput=null
SyslogIdentifier=lamp-update-writer
TimeoutStartSec=120
NoNewPrivileges=yes
PrivateNetwork=yes
ProtectSystem=strict
ReadWritePaths=/usr/local/lib/lampos /var/lib/lampos-update
ReadOnlyPaths=-/etc/lampOS/update
ProtectHome=yes
PrivateTmp=yes
";

/// Writes [`TEXT`] at [`UNIT`] under `root`, unless it is there already:
/// true when written. A link at the name (a masked unit is one) or any
/// other file that is not regular is refused and left as it is.
pub fn write(root: &Path) -> Result<bool, String> {
    let path = root.join(FOLDER).join(UNIT);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {
            if fs::read(&path).map_err(|error| format!("{UNIT}: {error}"))? == TEXT.as_bytes() {
                return Ok(false);
            }
        }
        Ok(_) => {
            return Err(format!(
                "{UNIT} is a link or not a regular file (masked?): it is left as it is"
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{UNIT}: {error}")),
    }
    write_whole(&path, TEXT.as_bytes(), 0o644).map_err(|error| format!("{UNIT}: {error}"))?;
    Ok(true)
}

/// Whether `show`, systemd's answer for [`PROPERTIES`] of [`UNIT`] after a
/// reload, is the unit [`write`] wrote: loaded, from `/etc/systemd/system`,
/// with no drop-in, and not masked.
pub fn loaded_as_written(show: &str) -> Result<(), String> {
    let mut properties = BTreeMap::new();
    for line in show.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line
            .split_once('=')
            .ok_or("the writer's unit: a malformed systemctl property")?;
        if !PROPERTIES.split(',').any(|name| name == key) {
            return Err(format!(
                "the writer's unit: an unexpected systemctl property {}",
                clean_line(key)
            ));
        }
        if properties.insert(key, value).is_some() {
            return Err(format!("the writer's unit: {key} twice"));
        }
    }
    let field = |name: &str| {
        properties
            .get(name)
            .copied()
            .ok_or_else(|| format!("the writer's unit: no {name}"))
    };
    let wanted = format!("/{FOLDER}/{UNIT}");
    let found = [
        ("Id", UNIT),
        ("LoadState", "loaded"),
        ("FragmentPath", wanted.as_str()),
        ("DropInPaths", ""),
    ];
    for (name, wanted) in found {
        let value = field(name)?;
        if value != wanted {
            return Err(format!(
                "the writer's unit: {name} is {:?}, not {wanted:?}",
                clean_line(value)
            ));
        }
    }
    // Started by a round alone: never enabled, so static or disabled. A
    // masked, linked, runtime or generated unit is not the one written.
    let state = field("UnitFileState")?;
    if !matches!(state, "static" | "disabled") {
        return Err(format!(
            "the writer's unit: UnitFileState is {:?}, not static",
            clean_line(state)
        ));
    }
    Ok(())
}

/// Writes the unit, reloads systemd if it was written, checks what systemd
/// loaded and starts it, waiting for its end. An error before the start
/// means nothing was run.
///
/// A start that ran past its bound only ends the waiting: systemd's job
/// goes on, and its result is read from what the command left behind, not
/// from this.
pub fn run(root: &Path, systemd: &mut dyn Systemd) -> Result<(), String> {
    if write(root)? {
        systemd.reload()?;
    }
    loaded_as_written(&systemd.writer_unit_properties()?)?;
    systemd.start_writer_unit()
}

#[cfg(test)]
mod tests;
