//! A part's other units, which a round carries beside its service (#3689,
//! item 2a, updater level 7; docs/updates.md, "A part's templates, sockets
//! and timers"): its templates (`lamp-<x>@.service`), sockets
//! (`lamp-<x>.socket`) and timers (`lamp-<x>.timer`).
//!
//! Each belongs to the part it starts through that part's `current` link:
//! a template by its own `ExecStart=`, a socket or a timer by the unit it
//! starts. One that starts anything else (the keeper's template, a fixed
//! path in `/usr/local/bin`; `lamp-update-back.timer`, whose service is the
//! fixed settings writer) belongs to no part, and no round carries it.
use crate::{manifest, unit};

/// The kinds of a part's other units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `lamp-<x>@.service`: written and reloaded, never restarted. Each
    /// instance a socket starts after the reload runs the new text.
    Template,
    /// `lamp-<x>.socket`: restarted if it was listening.
    Socket,
    /// `lamp-<x>.timer`: restarted if it was waiting.
    Timer,
}

/// The kind of a unit's name, when it is one of a part's other units.
pub fn kind(name: &str) -> Option<Kind> {
    let (stem, kind) = if let Some(stem) = name.strip_suffix("@.service") {
        (stem, Kind::Template)
    } else if let Some(stem) = name.strip_suffix(".socket") {
        (stem, Kind::Socket)
    } else if let Some(stem) = name.strip_suffix(".timer") {
        (stem, Kind::Timer)
    } else {
        return None;
    };
    manifest::is_binary_name(stem).then_some(kind)
}

/// The unit a socket or a timer starts, as systemd reads it: its
/// `Service=` or `Unit=`, else the template of its name for a socket that
/// accepts each connection, else the service of its name. None for a
/// template, or a name that is not one of a part's.
pub fn starts(name: &str, text: &str) -> Option<String> {
    let (stem, key) = match kind(name)? {
        Kind::Template => return None,
        Kind::Socket => (name.strip_suffix(".socket")?, "Service"),
        Kind::Timer => (name.strip_suffix(".timer")?, "Unit"),
    };
    let started = last(text, key).map_or_else(
        || {
            let accepts = kind(name) == Some(Kind::Socket)
                && last(text, "Accept")
                    .is_some_and(|value| matches!(value, "yes" | "true" | "on" | "1"));
            if accepts {
                format!("{stem}@.service")
            } else {
                format!("{stem}.service")
            }
        },
        str::to_owned,
    );
    (manifest::is_unit_name(&started) || kind(&started) == Some(Kind::Template)).then_some(started)
}

/// The part a unit of these kinds belongs to: the folder of its builds and
/// its binary, from the `ExecStart=` of the template, or of the unit the
/// socket or timer starts (its text from `text_of`). None when that does
/// not start a part through its `current` link, and for a socket that hands
/// its listener to one service rather than to a template's instance per
/// connection: systemd refuses to start such a socket again while its
/// service runs, so a round never restarts one.
pub fn part<'a>(
    name: &str,
    text: &str,
    text_of: impl Fn(&str) -> Option<&'a str>,
) -> Option<(String, String)> {
    let kind = kind(name)?;
    if kind == Kind::Template {
        return unit::linked(text);
    }
    let started = starts(name, text)?;
    // A socket starts a template's instances; a timer, a service.
    if (self::kind(&started) == Some(Kind::Template)) != (kind == Kind::Socket) {
        return None;
    }
    unit::linked(text_of(&started)?)
}

/// The last value a unit gives `key` (`Unit`), as systemd reads a key set
/// twice, with or without spaces around its `=`.
fn last<'t>(text: &'t str, key: &str) -> Option<&'t str> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(name, _)| name.trim() == key)
        .map(|(_, value)| value.trim())
        .next_back()
}

#[cfg(test)]
mod tests;
