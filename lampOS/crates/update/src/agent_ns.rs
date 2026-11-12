//! The shared piece of the lamp's agent namespace (#6510, "one namespace
//! for every engine"; #6530): what every engine's namespace gets alike,
//! used today by Hermes's and OpenClaw's (docs/install.md, "Harness's
//! hooks inside the agent namespace").
//!
//! Harness writes an engine's hooks as bare shell commands (`harness hook
//! ENGINE session-start`, found by `PATH`). Inside the namespace `/var/lib`
//! is an empty tmpfs, so `/usr/local/bin/harness` (a link into Harness's
//! state folder) leads nowhere and every hook would fail. Harness's state
//! is never shown there. Until Harness has a hook mode for a sandbox
//! (openharness#4194), [`HOOK_SHIM`] stands in for `harness` inside the
//! namespace ([`PATH_LINES`], first on its `PATH`): `harness hook …` is
//! answered with nothing and exit 0, anything else refused. It shows no
//! Harness state: it reads nothing. Each start of an engine's wrapper says
//! so once in the journal ([`hook_note_line`]), outside the namespace.
//!
//! [`HOOK_SHIM`] is root's, 0755, in [`BIN`] (root's, 0755), written with
//! an engine's namespace script and taken out with the last of them
//! ([`remove_hook_shim`]).
use crate::records::write_whole;
use std::{
    fs,
    io::ErrorKind,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// The folder put first on the `PATH` inside the namespace, root's, 0755,
/// beside lamp-update's own fixed files. Not hidden there: only `/run`,
/// `/var/lib` and `/etc/lampOS` are.
pub const BIN: &str = "/usr/local/lib/lampos/ns-bin";
/// The `harness` the namespace finds: [`HOOK_SHIM_TEXT`], root's, 0755.
pub const HOOK_SHIM: &str = "/usr/local/lib/lampos/ns-bin/harness";
/// [`HOOK_SHIM`]'s text: `harness hook …` answered with nothing and exit
/// 0, anything else refused with one line. It reads nothing.
pub const HOOK_SHIM_TEXT: &str = r#"#!/bin/sh
# lampOS: `harness` inside the lamp's agent namespace (#6530). Written by lamp-update.
# Harness's hooks can't reach Harness from inside (no CLI, no state, no credential; openharness#4194):
# `harness hook …` is answered with nothing and exit 0. Anything else is refused.
[ "${1-}" = hook ] && exit 0
echo "harness: not available inside the lamp's agent namespace" >&2
exit 1
"#;
/// What [`with_ns_path`] puts before a namespace script's last `exec`:
/// [`BIN`] first on the `PATH`, the caller's (or a plain one) after it.
pub const PATH_LINES: &str = "\
# The lamp's agent namespace (#6530): Harness's hooks find only the stand-in harness in /usr/local/lib/lampos/ns-bin,
# which answers them with nothing and shows no Harness state, until Harness has a sandbox hook mode (openharness#4194).
PATH=/usr/local/lib/lampos/ns-bin:${PATH:-/usr/local/bin:/usr/bin:/bin}
export PATH
";
/// The journal line each start of an engine's wrapper writes, once.
pub const HOOK_NOTE: &str =
    "Harness hooks aren't available inside the lamp's agent namespace yet (openharness#4194)";

/// The wrapper's line that writes [`HOOK_NOTE`] to the journal with
/// `tag`; no `logger`, or one failing, never fails the start.
pub fn hook_note_line(tag: &str) -> String {
    debug_assert!(
        !tag.is_empty()
            && tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "a plain tag"
    );
    format!("logger -t {tag} \"{HOOK_NOTE}\" 2>/dev/null || :\n")
}

/// `text` with `lines` put before its last line that starts with `exec `
/// (the one that runs the engine); unchanged when there is none.
fn before_last_exec(text: &str, lines: &str) -> String {
    let at = if text.starts_with("exec ") {
        Some(0)
    } else {
        text.rfind("\nexec ").map(|at| at + 1)
    };
    match at {
        Some(at) => format!("{}{lines}{}", &text[..at], &text[at..]),
        None => text.to_owned(),
    }
}

/// A namespace script (or a wrapper's way into a running namespace) with
/// [`PATH_LINES`] before its last `exec`, so what it runs finds
/// [`HOOK_SHIM`] as `harness`.
pub fn with_ns_path(text: &str) -> String {
    before_last_exec(text, PATH_LINES)
}

/// A wrapper with [`hook_note_line`]`(tag)` before its first line that
/// sets `LAMP_NS_UID` (after the token is read, outside the namespace);
/// unchanged when there is none.
pub fn with_hook_note(text: &str, tag: &str) -> String {
    let line = hook_note_line(tag);
    match text.find("\nLAMP_NS_UID=") {
        Some(at) => format!("{}{line}{}", &text[..=at], &text[at + 1..]),
        None => text.to_owned(),
    }
}

fn on(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

/// [`HOOK_SHIM`] there with this build's text, a regular file, not
/// followed through a link.
pub fn hook_shim_there(root: &Path) -> bool {
    let place = on(root, HOOK_SHIM);
    fs::symlink_metadata(&place).is_ok_and(|meta| meta.is_file())
        && fs::read(&place).is_ok_and(|bytes| bytes == HOOK_SHIM_TEXT.as_bytes())
}

/// [`BIN`] made (root's, 0755; its folder must be there, and anything else
/// in its place is refused) and [`HOOK_SHIM`] written whole beside and
/// renamed in, root's, 0755.
pub fn write_hook_shim(root: &Path) -> Result<(), String> {
    let bin = on(root, BIN);
    match fs::symlink_metadata(&bin) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(format!("{BIN}: not a folder")),
        Err(error) if error.kind() == ErrorKind::NotFound => fs::DirBuilder::new()
            .mode(0o755)
            .create(&bin)
            .map_err(|error| format!("{BIN}: {error}"))?,
        Err(error) => return Err(format!("{BIN}: {error}")),
    }
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("{BIN}: {error}"))?;
    write_whole(&on(root, HOOK_SHIM), HOOK_SHIM_TEXT.as_bytes(), 0o755)
        .map_err(|error| format!("{HOOK_SHIM}: {error}"))
}

/// [`HOOK_SHIM`] and [`BIN`] (when empty) taken out, unless one of
/// `scripts` (every engine's namespace script) is still there.
pub fn remove_hook_shim(root: &Path, scripts: &[&str], say: &mut dyn FnMut(&str)) {
    if scripts
        .iter()
        .any(|script| fs::symlink_metadata(on(root, script)).is_ok())
    {
        return;
    }
    match fs::remove_file(on(root, HOOK_SHIM)) {
        Ok(()) => say(&format!("{HOOK_SHIM} taken out")),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => say(&format!("{HOOK_SHIM} not taken out: {error}")),
    }
    let _ = fs::remove_dir(on(root, BIN));
}

#[cfg(test)]
mod tests;
