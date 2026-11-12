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
//! - started in a skill run's own folder (`pwd -P` exactly
//!   [`hermes_install::RUN_FOLDERS`]`/skill-<run-id>`, a plain run id;
//!   #6510 Part 1 step 2), reads the run's one-run token from
//!   [`RUNS`]`/<run-id>` once, deletes the file, and passes the token on in
//!   the environment only, as [`TOKEN_ENV`], never in argv, with
//!   lamp-control's socket and the SDK's `PYTHONPATH`; the run also gets a
//!   network namespace of its own (`--net`, `LAMP_NS_NET=run`), as Hermes's
//!   skill runs ([`hermes_install::RUN_LINES`], the same lines). No token
//!   ends it before anything runs. In any other folder: no token and no
//!   `--net`, whatever the caller's environment says;
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
//! onto this one in step 2. A round calls [`write_engine`] for every other
//! engine Harness says is installed, with `LAMP_AGENT_NS=on`
//! ([`super::rounds`]).
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
/// ([`hermes_install::RUNS`]).
pub const RUNS: &str = hermes_install::RUNS;
/// The variable the engine gets the run's token in: the one a skill's run
/// already reads (`lamp_tasks::skill_task::TOKEN_ENV`).
pub const TOKEN_ENV: &str = "LAMP_CONTROL_TOKEN";
/// The longest engine name.
pub const ENGINE_MAX: usize = 32;

/// An engine name is plain: `a-z`, `0-9` and `-`, 1 to [`ENGINE_MAX`]
/// bytes, never starting with `-`, so it is one file name and one word in
/// a script, never an option.
pub fn engine_ok(engine: &str) -> bool {
    !engine.is_empty()
        && engine.len() <= ENGINE_MAX
        && engine
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
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
/// looked for by name on `PATH`, never by a path. A skill run's token file
/// is read and deleted first ([`hermes_install::run_lines`]), so a wrapper
/// that fails later (no engine program found) leaves none behind; the
/// token is read into a shell variable (never a command's argument) and
/// exported alone.
const WRAPPER_TEMPLATE: &str = concat!(
    r#"#!/bin/sh
# lampOS: @E@ on the lamp, in the one agent namespace (#6510). Written by lamp-update.
# lampOS/docs/install.md, "One agent namespace for every engine". It runs @E@'s own program, the first `@E@` on the
# caller's PATH that is not a lamp-update wrapper, as its caller (lampos, Harness's user), in a user, mount and PID
# namespace (/usr/local/lib/lampos/agent-ns) with HOME /var/lib/lampos-agents/@E@: no lamp service's files or
# sockets, no camera, mic or serial device, no lamp process.
E=@E@
fail() { echo "$E: $1" >&2; exit 1; }
unset LAMP_CONTROL_TOKEN LAMP_NS_NET
"#,
    hermes_install::run_lines!(),
    r#"N=
[ "${LAMP_NS_NET-}" != run ] || N=--net
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
LAMP_NS_UID=$(id -u) && LAMP_NS_GID=$(id -g) || exit 1
LAMP_NS_ENGINE=$E LAMP_NS_ENGINE_BIN=$X
export LAMP_NS_UID LAMP_NS_GID LAMP_NS_ENGINE LAMP_NS_ENGINE_BIN
exec /usr/bin/unshare --map-root-user $N --mount --pid --kill-child --mount-proc --propagation private /usr/local/lib/lampos/agent-ns "$@"
"#
);

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

/// Whether the file at `engine`'s [`wrapper_path`] under `root` is a
/// regular file (not a link) holding exactly [`wrapper_text`]`(engine)`:
/// one this build wrote, and nothing else. Refused unless `engine` is
/// plain.
pub fn ours(root: &Path, engine: &str) -> bool {
    match (wrapper_path(engine), wrapper_text(engine)) {
        (Ok(wrapper), Ok(text)) => holds(root, &wrapper, &text),
        _ => false,
    }
}

/// The engines whose wrapper under `root` is [`ours`]: each plain name in
/// [`WRAPPERS`], sorted. A file of any other text there (the engine's own
/// program, a link, an older build's wrapper) is never one of them.
pub fn ours_written(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(on(root, WRAPPERS)) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| engine_ok(name) && ours(root, name))
        .collect();
    found.sort();
    found
}

/// `engine`'s wrapper under `root` taken out, only when it is [`ours`]:
/// `Ok(true)` when taken out, `Ok(false)` when it is not ours (left as it
/// is) or not there. Its home and [`NS`] are left: the home may hold the
/// engine's own data, and [`NS`] is shared.
pub fn remove_ours(root: &Path, engine: &str) -> Result<bool, String> {
    if !ours(root, engine) {
        return Ok(false);
    }
    let wrapper = wrapper_path(engine)?;
    match fs::remove_file(on(root, &wrapper)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("{wrapper}: {error}")),
    }
}

/// The longest `lampos`'s login-shell lookup may take, in seconds; past
/// it nothing is found, so the engine is not present.
pub const LOOKUP_SECS: &str = "5";

/// How Harness finds `engine`: `command -v` in `lampos`'s login shell
/// (`runuser -l`, which reads the same rc files Harness's installer
/// writes, `~/.local/bin` before `/usr/local/bin`), bounded by
/// `timeout` ([`LOOKUP_SECS`]) so a slow rc file never holds a round.
/// Refused unless `engine` is plain, so it is one word in the command.
pub fn login_lookup_argv(engine: &str) -> Result<Vec<String>, String> {
    plain(engine)?;
    Ok([
        "timeout",
        LOOKUP_SECS,
        "runuser",
        "-l",
        crate::install::USER,
        "-c",
    ]
    .map(str::to_owned)
    .into_iter()
    .chain([format!("command -v {engine}")])
    .collect())
}

/// The program `lampos`'s login shell finds for `engine`
/// ([`login_lookup_argv`]), or `None` when it finds none or cannot be
/// asked.
pub fn login_finds(engine: &str) -> Option<String> {
    let argv = login_lookup_argv(engine).ok()?;
    let out = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let found = String::from_utf8(out.stdout).ok()?;
    let found = found.trim();
    (!found.is_empty()).then(|| found.to_owned())
}

/// The fixed text of [`login_programs_argv`]'s look, after `E=<engine>;`:
/// each full folder of `lampos`'s login `PATH` in order, every executable
/// regular `<dir>/<engine>` printed, one a line, except
/// [`WRAPPERS`]`/<engine>` and any file carrying [`MARK`] in its first
/// [`MARK_WITHIN`] bytes (a lamp-update wrapper, as the wrapper itself
/// skips them). Ends 0 when it walked the whole `PATH`.
const PROGRAMS_LOOK: &str = r#"o=$IFS; set -f; IFS=:; for d in ${PATH-}; do IFS=$o; case $d in /*) ;; *) continue ;; esac; [ "$d/$E" = "/usr/local/bin/$E" ] && continue; [ -f "$d/$E" ] && [ -x "$d/$E" ] || continue; head -c 512 "$d/$E" | grep -qF "Written by lamp-update." && continue; printf '%s\n' "$d/$E"; done; exit 0"#;

/// The engine's own programs on `lampos`'s login `PATH` ([`PROGRAMS_LOOK`]),
/// asked as [`login_lookup_argv`] asks: `lampos`'s login shell, bounded by
/// `timeout` ([`LOOKUP_SECS`]). Refused unless `engine` is plain, so it is
/// one word in the command; the rest of the text is fixed.
pub fn login_programs_argv(engine: &str) -> Result<Vec<String>, String> {
    plain(engine)?;
    Ok([
        "timeout",
        LOOKUP_SECS,
        "runuser",
        "-l",
        crate::install::USER,
        "-c",
    ]
    .map(str::to_owned)
    .into_iter()
    .chain([format!("E={engine}; {PROGRAMS_LOOK}")])
    .collect())
}

/// The lines of [`PROGRAMS_LOOK`]'s output that are its programs: full
/// paths ending in `/<engine>`. Anything else a login shell's rc files
/// print is passed over.
pub fn programs_in(engine: &str, said: &str) -> Vec<String> {
    let tail = format!("/{engine}");
    said.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('/') && line.ends_with(&tail))
        .map(str::to_owned)
        .collect()
}

/// `engine`'s own programs on `lampos`'s login `PATH`
/// ([`login_programs_argv`]): `Ok` with none only when the look ran to the
/// end (exit 0) and found none. A look that cannot be run (no `timeout`
/// or `runuser`), fails or runs past [`LOOKUP_SECS`] is `Err`: nothing is
/// known, so nothing is taken out for it.
pub fn login_programs(engine: &str) -> Result<Vec<String>, String> {
    let argv = login_programs_argv(engine)?;
    let out = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("{}: {error}", argv[0]))?;
    if !out.status.success() {
        return Err(match out.status.code() {
            Some(124) => format!("lampos's login shell gave no answer within {LOOKUP_SECS} s"),
            Some(code) => format!("lampos's login shell ended {code}"),
            None => "lampos's login shell was stopped".to_owned(),
        });
    }
    Ok(programs_in(engine, &String::from_utf8_lossy(&out.stdout)))
}

/// Whether what `lampos`'s login shell finds for `engine` (`found`, from
/// [`login_finds`]) is the wrapper itself. An engine's own program earlier
/// on that `PATH` (`~/.local/bin/<engine>` from a native installer or a
/// user-prefix npm or pip) would run with no namespace, so the wrapper
/// there is not enough (Heron's read of #6596).
pub fn found_first(engine: &str, found: Option<&str>) -> bool {
    wrapper_path(engine).is_ok_and(|wrapper| found == Some(wrapper.as_str()))
}

/// `engine`'s entry points there as this build writes them for `shown`:
/// the wrapper and [`NS`] with this build's text, the hook stand-in, and
/// the engine's home a folder, and the wrapper the `engine` `lampos`'s
/// login shell finds (`found`, [`found_first`]). Anything else is the
/// engine missing: written again, and not offered while something else
/// is found first.
pub fn present(root: &Path, engine: &str, shown: Shown, found: Option<&str>) -> bool {
    let (Ok(wrapper), Ok(text), Ok(home)) =
        (wrapper_path(engine), wrapper_text(engine), home_of(engine))
    else {
        return false;
    };
    found_first(engine, found)
        && holds(root, &wrapper, &text)
        && holds(root, NS, &ns_text(shown))
        && super::hook_shim_there(root)
        && fs::symlink_metadata(on(root, &home)).is_ok_and(|meta| meta.is_dir())
}

#[cfg(test)]
mod tests;
