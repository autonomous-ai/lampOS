//! One agent namespace for every engine (#6510 Part 1, step 1; the dev's
//! "Generic wrapper"; docs/install.md, "One agent namespace for every
//! engine"): for each engine Harness reports on the lamp, lamp-update
//! writes [`wrapper_text`]`(engine)` as `/usr/local/bin/<engine>`
//! ([`wrapper_path`]), and all of them run the one namespace script
//! [`NS`] ([`ns_text`]). No engine is named in either: the engine is data,
//! a plain name ([`engine_ok`]).
//!
//! **The wrapper** (root's, 0755; [`MARK`] in its first 512 bytes, which
//! lamp-tasks' `LAMP_WRAPPER_MARK` (#6506) looks for):
//! - finds the engine's own program on the caller's `PATH` (`lampos`'s,
//!   as Harness starts it): the first `<engine>` there that is not this
//!   wrapper and carries no [`MARK`] (so never another lamp-update wrapper
//!   or the hook stand-in). No path to it is written in any text;
//! - with [`RUN_ID_ENV`] set (a skill's run), reads the run's one-run token
//!   from [`RUNS`]`/<run-id>` once, deletes the file, and passes the token
//!   on in the environment only, as [`TOKEN_ENV`], never in argv; the run
//!   also gets a network namespace of its own (`--net`, `LAMP_NS_NET=run`,
//!   as Hermes's skill runs). A run id that is not plain, or no token, ends
//!   it before anything runs;
//! - writes the hook stand-in's journal line ([`super::with_hook_note`]);
//! - runs [`NS`] in a new user, mount and PID namespace with its own
//!   `/proc`, as Hermes's wrapper does.
//!
//! **The namespace script** is Hermes's ([`hermes_install::ns_text`], the
//! same allow-list, switches and skill-run network) with the engine made
//! data ([`NS_EDITS`]): `HOME` is the engine's own folder
//! [`HOMES`]`/<engine>` (0700, `lampos`'s; [`home_of`]), the program the
//! wrapper found is bound back read-only at its own path, and it runs that
//! program. The hook stand-in ([`super::HOOK_SHIM`]) is first on its `PATH`
//! ([`super::with_ns_path`]). The lampos SDK (#6507) is under `/usr`, which
//! the namespace leaves as it is.
//!
//! Hermes and OpenClaw keep their own wrappers and namespace scripts for
//! now ([`crate::hermes_install`], [`crate::openclaw_install`]); they move
//! onto this one in step 2. Nothing calls [`write_engine`] from a round
//! yet: that, for every engine Harness reports, is the next piece.
use crate::{
    hermes_install::{self, Shown, holds, on},
    records::write_whole,
};
use std::{
    fs,
    io::{ErrorKind, Read},
    os::unix::fs::{DirBuilderExt, PermissionsExt, chown},
    path::Path,
};

/// The mark every lamp-update wrapper carries in its first [`MARK_WITHIN`]
/// bytes (lamp-tasks' `LAMP_WRAPPER_MARK`, #6506).
pub const MARK: &str = "Written by lamp-update.";
/// How far into a wrapper [`MARK`] is looked for.
pub const MARK_WITHIN: usize = 512;
/// The folder each engine's wrapper is written in, as `<engine>`: on
/// `lampos`'s login `PATH`, where Harness finds an engine by `command -v`.
pub const WRAPPERS: &str = "/usr/local/bin";
/// The one namespace script every engine's wrapper runs, root's, 0755.
pub const NS: &str = "/usr/local/lib/lampos/agent-ns";
/// The engines' homes, root's, 0755; each engine's `<engine>` in it is
/// `lampos`'s, 0700 ([`home_of`]).
pub const HOMES: &str = "/var/lib/lampos-agents";
/// Where lamp-tasks leaves a run's one-run token, as `<run-id>`
/// (`lamp_tasks::run_token_file::RUNS_DIR`: tmpfs, 0600 in `lampos`'s
/// 0700 folder; the wrapper runs as `lampos` too).
pub const RUNS: &str = "/run/lampos-tasks/runs";
/// The variable that names a skill's run to the wrapper. **Provisional**:
/// Harness passes an engine only a fixed list of variables today
/// (openharness `hold.rs`), so this needs Harness to pass it, or the run
/// found another way (its folder), before skill runs go through Harness
/// (#6510 Part 1 step 2).
pub const RUN_ID_ENV: &str = "LAMP_RUN_ID";
/// The variable the engine gets the run's token in: the one a skill's run
/// already reads (`lamp_tasks::skill_task::TOKEN_ENV`).
pub const TOKEN_ENV: &str = "LAMP_CONTROL_TOKEN";
/// The longest engine name.
pub const ENGINE_MAX: usize = 32;

/// An engine name is plain: `a-z`, `0-9` and `-`, 1 to [`ENGINE_MAX`]
/// bytes, so it is one file name and one word in a script.
pub fn engine_ok(engine: &str) -> bool {
    !engine.is_empty()
        && engine.len() <= ENGINE_MAX
        && engine
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn plain(engine: &str) -> Result<(), String> {
    if engine_ok(engine) {
        Ok(())
    } else {
        Err(format!(
            "{engine:?}: not a plain engine name (a-z, 0-9, -, at most {ENGINE_MAX})"
        ))
    }
}

/// The engine's wrapper, `/usr/local/bin/<engine>`.
pub fn wrapper_path(engine: &str) -> Result<String, String> {
    plain(engine)?;
    Ok(format!("{WRAPPERS}/{engine}"))
}

/// The engine's `HOME` inside the namespace, `/var/lib/lampos-agents/<engine>`.
pub fn home_of(engine: &str) -> Result<String, String> {
    plain(engine)?;
    Ok(format!("{HOMES}/{engine}"))
}

/// [`wrapper_text`]'s text, `@E@` the engine. The engine's program is
/// looked for by name on `PATH`, never by a path. The token is read into a
/// shell variable (never a command's argument) and exported alone.
const WRAPPER_TEMPLATE: &str = r#"#!/bin/sh
# lampOS: @E@ on the lamp, in the one agent namespace (#6510). Written by lamp-update.
# lampOS/docs/install.md, "One agent namespace for every engine". It runs @E@'s own program, the first `@E@` on the
# caller's PATH that is not a lamp-update wrapper, as its caller (lampos, Harness's user), in a user, mount and PID
# namespace (/usr/local/lib/lampos/agent-ns) with HOME /var/lib/lampos-agents/@E@: no lamp service's files or
# sockets, no camera, mic or serial device, no lamp process. A skill's run (LAMP_RUN_ID, provisional) gets its
# one-run token from /run/lampos-tasks/runs/<run-id>, read once and deleted, in its environment only.
E=@E@
fail() { echo "$E: $1" >&2; exit 1; }
I=$(readlink -f "$0") || fail "this wrapper is not found"
X=
o=$IFS
set -f
IFS=:
for d in ${PATH-}; do
  IFS=$o
  case $d in /*) ;; *) continue ;; esac
  [ -f "$d/$E" ] && [ -x "$d/$E" ] || continue
  r=$(readlink -f "$d/$E") || continue
  [ "$r" != "$I" ] || continue
  head -c 512 "$r" | grep -qF "Written by lamp-update." && continue
  X=$r
  break
done
IFS=$o
set +f
[ -n "$X" ] || fail "no $E on PATH besides lamp-update's wrappers"
N=
unset LAMP_CONTROL_TOKEN
if [ -n "${LAMP_RUN_ID+set}" ]; then
  case $LAMP_RUN_ID in ''|*[!abcdefghijklmnopqrstuvwxyz0123456789-]*) fail "LAMP_RUN_ID is not a plain run id" ;; esac
  [ ${#LAMP_RUN_ID} -le 64 ] || fail "LAMP_RUN_ID is not a plain run id"
  F=/run/lampos-tasks/runs/$LAMP_RUN_ID
  T=
  if [ -f "$F" ] && [ ! -L "$F" ]; then T=$(cat -- "$F") || T=; fi
  rm -f -- "$F" || fail "the run's token file is not deleted"
  [ -n "$T" ] || fail "no token for run $LAMP_RUN_ID"
  LAMP_CONTROL_TOKEN=$T
  unset T
  export LAMP_CONTROL_TOKEN
  LAMP_NS_NET=run
  export LAMP_NS_NET
  N=--net
else
  unset LAMP_NS_NET
fi
unset LAMP_RUN_ID
LAMP_NS_UID=$(id -u) && LAMP_NS_GID=$(id -g) || exit 1
LAMP_NS_ENGINE=$E LAMP_NS_ENGINE_BIN=$X
export LAMP_NS_UID LAMP_NS_GID LAMP_NS_ENGINE LAMP_NS_ENGINE_BIN
exec /usr/bin/unshare --map-root-user $N --mount --pid --kill-child --mount-proc --propagation private /usr/local/lib/lampos/agent-ns "$@"
"#;

/// The engine's wrapper ([`wrapper_path`]): [`WRAPPER_TEMPLATE`] for
/// `engine`, with the hook stand-in's journal line tagged with it
/// ([`super::with_hook_note`]). Refused unless `engine` is plain.
pub fn wrapper_text(engine: &str) -> Result<String, String> {
    plain(engine)?;
    Ok(super::with_hook_note(
        &WRAPPER_TEMPLATE.replace("@E@", engine),
        engine,
    ))
}

/// Hermes's namespace header ([`hermes_install::NS_TEXT`] before its first
/// command), replaced whole by [`NS_HEADER`].
fn hermes_header() -> &'static str {
    let text = hermes_install::NS_TEXT;
    &text[..text.find("read -r _ _ count").unwrap_or(0)]
}

/// [`NS`]'s own header.
const NS_HEADER: &str = "\
#!/bin/sh
# lampOS: the one agent namespace every engine's wrapper in /usr/local/bin makes (#6510; lampOS/docs/install.md,
# \"One agent namespace for every engine\"). Written by lamp-update. Root here only: it shows the engine only its own
# home /var/lib/lampos-agents/<engine>, its own program (read-only), its work folder, the skills (read-only) and
# resolv.conf over empty read-only /run, /var/lib and /etc/lampOS, gives it its own /tmp, hides the camera, mic and
# serial devices, then drops back to the caller's own ids in a nested user namespace,
# where these mounts are locked. Any step failing ends it before the engine runs.
";

/// What [`ns_text`] changes in Hermes's script, in order, each found
/// exactly once (a test holds it for every switch): the engine and its
/// program taken from the wrapper (each checked again here: plain, and a
/// full path to a file), `HOME` the engine's own, the program staged with
/// the rest, moved back at its own path and made read-only, and run.
pub const NS_EDITS: [(&str, &str); 8] = [
    (
        "only inside the namespace /usr/local/bin/hermes makes",
        "only inside the namespace an engine's wrapper makes",
    ),
    (
        "H=/var/lib/lampos-hermes ",
        "E=${LAMP_NS_ENGINE-} X=${LAMP_NS_ENGINE_BIN-}\nH=/var/lib/lampos-agents/$E ",
    ),
    (
        "fail() { echo \"hermes-ns: $1\" >&2; exit 1; }\n",
        "fail() { echo \"agent-ns: $1\" >&2; exit 1; }\n\
         case $E in ''|*[!abcdefghijklmnopqrstuvwxyz0123456789-]*) fail \"LAMP_NS_ENGINE is not a plain engine name\" ;; esac\n\
         [ ${#E} -le 32 ] || fail \"LAMP_NS_ENGINE is not a plain engine name\"\n\
         case $X in /*) ;; *) fail \"LAMP_NS_ENGINE_BIN is not a full path\" ;; esac\n\
         [ -f \"$X\" ] || fail \"no program at $X\"\n",
    ),
    (
        "if [ -d \"$K\" ]; then mount --bind \"$K\" \"$S/k\" || fail \"no $K\"; else K=; fi\n",
        "if [ -d \"$K\" ]; then mount --bind \"$K\" \"$S/k\" || fail \"no $K\"; else K=; fi\n\
         touch \"$S/x\" && mount --bind \"$X\" \"$S/x\" || fail \"no $X\"\n",
    ),
    (
        "&& mount --move \"$S/k\" \"$K\" && ro \"$K\" || fail \"$K not shown\"; fi\n",
        "&& mount --move \"$S/k\" \"$K\" && ro \"$K\" || fail \"$K not shown\"; fi\n\
         mkdir -p \"${X%/*}\" && { [ -e \"$X\" ] || touch \"$X\"; } && mount --move \"$S/x\" \"$X\" && ro \"$X\" || fail \"$X not shown\"\n",
    ),
    ("rm -f \"$S/r\"", "rm -f \"$S/r\" \"$S/x\""),
    (
        "unset LAMP_NS_UID LAMP_NS_GID\n",
        "unset LAMP_NS_UID LAMP_NS_GID LAMP_NS_ENGINE LAMP_NS_ENGINE_BIN\n",
    ),
    (
        " /var/lib/lampos-hermes/.local/bin/hermes \"$@\"\n",
        " \"$X\" \"$@\"\n",
    ),
];

/// [`NS`]'s text for what it shows: Hermes's ([`hermes_install::ns_text`]
/// for `shown`, the same allow-list and switches) with [`NS_HEADER`],
/// [`NS_EDITS`], its messages as `agent-ns` and its words about the engine
/// made general, and the hook stand-in first on its `PATH`
/// ([`super::with_ns_path`]; once only, when Hermes's text already has it).
/// It names no engine.
pub fn ns_text(shown: Shown) -> String {
    let mut text = hermes_install::ns_text(shown.skills, shown.tasks);
    text = text.replacen(hermes_header(), NS_HEADER, 1);
    for (from, to) in NS_EDITS {
        text = text.replacen(from, to, 1);
    }
    text = text
        .replace("hermes-ns: ", "agent-ns: ")
        .replace(
            "LAMP_HERMES_SKILLS=on (#6128)",
            "The skills switch on (#6128)",
        )
        .replace("Hermes", "the engine");
    if text.contains(super::PATH_LINES) {
        text
    } else {
        super::with_ns_path(&text)
    }
}

/// Whether the file at `place` is a regular file (not a link) carrying
/// [`MARK`] in its first [`MARK_WITHIN`] bytes.
pub fn marked(place: &Path) -> bool {
    if !fs::symlink_metadata(place).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let mut head = Vec::with_capacity(MARK_WITHIN);
    fs::File::open(place)
        .and_then(|file| file.take(MARK_WITHIN as u64).read_to_end(&mut head))
        .is_ok()
        && head
            .windows(MARK.len())
            .any(|window| window == MARK.as_bytes())
}

/// A folder under `root` made with `mode` when missing; one there must be
/// a folder, not a link.
fn folder(root: &Path, path: &str, mode: u32) -> Result<(), String> {
    let place = on(root, path);
    match fs::symlink_metadata(&place) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(format!("{path}: not a folder")),
        Err(error) if error.kind() == ErrorKind::NotFound => fs::DirBuilder::new()
            .mode(mode)
            .create(&place)
            .map_err(|error| format!("{path}: {error}"))?,
        Err(error) => return Err(format!("{path}: {error}")),
    }
    fs::set_permissions(&place, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("{path}: {error}"))
}

/// `engine`'s entry points under `root`, for what the namespace shows:
/// [`HOMES`] (root's, 0755) and the engine's home in it (`owner`'s, 0700:
/// `lampos`'s uid and gid), [`NS`], the hook stand-in
/// ([`super::write_hook_shim`]), then the wrapper last, each root's, 0755,
/// written whole beside and renamed in, so `lampos` never finds a wrapper
/// before what it runs. A file at the wrapper's place without [`MARK`]
/// (the engine's own program, or anyone else's) is refused, never
/// replaced. Refused unless `engine` is plain.
pub fn write_engine(
    root: &Path,
    engine: &str,
    owner: (u32, u32),
    shown: Shown,
) -> Result<(), String> {
    let wrapper = wrapper_path(engine)?;
    let home = home_of(engine)?;
    let place = on(root, &wrapper);
    if fs::symlink_metadata(&place).is_ok() && !marked(&place) {
        return Err(format!("{wrapper}: not lamp-update's; left as it is"));
    }
    folder(root, HOMES, 0o755)?;
    folder(root, &home, 0o700)?;
    chown(on(root, &home), Some(owner.0), Some(owner.1))
        .map_err(|error| format!("{home}: {error}"))?;
    write_whole(&on(root, NS), ns_text(shown).as_bytes(), 0o755)
        .map_err(|error| format!("{NS}: {error}"))?;
    super::write_hook_shim(root)?;
    write_whole(&place, wrapper_text(engine)?.as_bytes(), 0o755)
        .map_err(|error| format!("{wrapper}: {error}"))
}

/// `engine`'s entry points there as this build writes them for `shown`:
/// the wrapper and [`NS`] with this build's text, the hook stand-in, and
/// the engine's home a folder. Anything else is the engine missing,
/// written again.
pub fn present(root: &Path, engine: &str, shown: Shown) -> bool {
    let (Ok(wrapper), Ok(text), Ok(home)) =
        (wrapper_path(engine), wrapper_text(engine), home_of(engine))
    else {
        return false;
    };
    holds(root, &wrapper, &text)
        && holds(root, NS, &ns_text(shown))
        && super::hook_shim_there(root)
        && fs::symlink_metadata(on(root, &home)).is_ok_and(|meta| meta.is_dir())
}

#[cfg(test)]
mod tests;
