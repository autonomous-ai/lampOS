//! Hermes put on the lamp after Harness (#5978; docs/install.md, "Hermes on
//! the lamp"): off on every lamp unless `LAMP_HERMES_ON_LAMP=on` in
//! lamp-update's own unit ([`PRODUCT_DEFAULT`] is off: the product turns
//! it on only once its lamp-c0a4 numbers are judged, the first builder,
//! 10 Nov); `off` there keeps it off.
//! It rides on Harness before the sign-in ([`crate::harness_install`]):
//! with that off, Hermes is off as well.
//!
//! Hermes (NousResearch hermes-agent) is a coding agent Harness already
//! knows as an engine. Harness finds an engine only by `command -v` in its
//! user's login shell, so a `hermes` on `lampos`'s login `PATH`
//! ([`WRAPPER`], in `/usr/local/bin`) is all it needs.
//!
//! **As `lampos`, in a namespace.** Hermes runs as `lampos`, Harness's own
//! user, so Harness's hooks and its reading of Hermes's sessions work.
//! Hermes runs shell commands, and as plain `lampos` they could read
//! lamp-cloud's relay key (its systemd credential, and lamp-cloud's state
//! folder). So [`WRAPPER`] never starts Hermes but through
//! `unshare --map-root-user --mount --pid --kill-child --mount-proc` (its
//! own PID namespace and `/proc`, so no lamp process, lamp-cloud's
//! `/proc/<pid>/root` among them, is visible) and [`NS`], which lays an empty
//! read-only tmpfs over each of [`HIDDEN`] and then drops back to the
//! caller's own ids in a nested user namespace, where those mounts are
//! locked. Hermes is given only lamp-cloud's local forwarder token
//! ([`TOKEN`], as [`TOKEN_ENV`]), never the relay key. Before anything is
//! installed, the namespace is tried as `lampos` ([`userns_argv`]); a lamp
//! where it does not work gets nothing, `no_userns`. No unit, no daemon,
//! nothing at boot: Harness starts `hermes` in a pane only when a task is
//! sent to it.
//!
//! **When.** After a `serve` round that reached the channel, with the
//! switch on and Hermes missing ([`present`]): the round writes [`ASK`].
//! With Harness there, it starts Harness's one-shot
//! ([`crate::harness_install::UNIT`]) itself; with Harness missing, the
//! one-shot Harness's own round starts installs Hermes right after
//! Harness. A Hermes failure never fails Harness's install or the round:
//! it is recorded in [`RECORD`], its entry points ([`WRAPPER`], [`NS`])
//! taken out, and it is tried again [`RETRY_AFTER`] later. With the switch
//! off, a round that finds an entry point asks the one-shot to take them
//! out ([`after_round_off`]); Hermes's folder stays.
//!
//! **Pinned.** The installer of hermes-agent's tag `v0.22.2`
//! ([`INSTALLER_ORIGIN`]) is compiled in ([`INSTALLER`], a byte-for-byte
//! copy committed at `deploy/hermes/`) and checked by its SHA-256 before
//! anything runs, and again after it is written to a file (`pin_mismatch`).
//! It runs as `lampos` once per stage of [`STAGES`], each with `--branch
//! main --commit` [`COMMIT`], the installer's own pin: it clones, checks
//! out that commit detached and builds from it, never from a release it
//! looks up. After it, the checkout's `HEAD` is read; another commit is
//! `version`. Hermes's own update check is turned off in its config
//! ([`CONFIG`]), so the pin moves only with lampOS: a new Hermes is a PR
//! that copies the installer again and updates [`COMMIT`], [`VERSION`] and
//! [`INSTALLER_SHA256`]; tests hold them to the file. A Hermes at another
//! commit found in [`PLACE`] is moved aside to [`ASIDE`] before the
//! install, and removed once the new one runs.
//!
//! **Python.** The lamp has no Python. The installer's `venv` stage fetches
//! its own pinned `uv` (checked by its SHA-256 in the checkout's
//! `pm/lock.json`) and a uv-managed Python, both under [`PLACE`], and its
//! package manager installs the dependencies from the checkout's
//! hash-locked `uv.lock`.
use crate::{
    harness_install,
    install::{self, Runner, USER},
    records::write_whole,
    world::Systemd,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, ErrorKind, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The switch, in lamp-update's own unit: `on` exactly.
pub const SWITCH: &str = "LAMP_HERMES_ON_LAMP";
/// Whether a lamp installed as the product installs Hermes with [`SWITCH`]
/// unset: off, until the lamp's Hermes numbers on lamp-c0a4 (RAM at rest
/// and while running a task, CPU, the voice's first answer, free disk on
/// 16 GB) are judged (the first builder, 10 Nov). Any other lamp with it
/// unset is off too.
pub const PRODUCT_DEFAULT: bool = false;

/// Where the install is, in lamp-update's records (0600), next to
/// Harness's [`harness_install::RECORD`]. lamp-update's own: not said on
/// `update/status` yet.
pub const RECORD: &str = "hermes-install.json";
/// What the round hands the one-shot: install Hermes, or take its entry
/// points out ([`Ask::remove`]).
pub const ASK: &str = "hermes-ask.json";
/// One Hermes install at a time.
pub const LOCK: &str = "hermes.lock";
/// An `installing` older than this is taken as ended without a word (the
/// one-shot killed past its `TimeoutStartSec`, an hour, or the lamp lost
/// power) and tried again.
pub const STUCK_AFTER: Duration = Duration::from_secs(70 * 60);
/// A failure the same steps would likely repeat is tried again by the
/// first round this long after it, not after every round; `serve` and
/// `not_started` are tried at the next round.
pub const RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Hermes's folder: `lampos`'s, 0700, the `HOME` its installer runs with.
/// Its checkout, its `uv`, its Python and its own home are all under it.
pub const PLACE: &str = "/var/lib/lampos-hermes";
/// Where a Hermes of another commit found in [`PLACE`] is moved before an
/// install, and removed from once the new one runs.
pub const ASIDE: &str = "/var/lib/lampos-hermes.old";
/// Hermes's own home (`HERMES_HOME`): its config, its sessions, its
/// checkout ([`CHECKOUT`]). lamp-tasks' `runtime::HERMES_HOME` and
/// Harness's hold file name the same folder.
pub const HERMES_HOME: &str = "/var/lib/lampos-hermes/.hermes";
/// The installer's checkout (`$HERMES_HOME/hermes-agent`).
pub const CHECKOUT: &str = "/var/lib/lampos-hermes/.hermes/hermes-agent";
/// The `hermes` command the installer publishes, in [`PLACE`]'s
/// `.local/bin`. Never on any `PATH`: only [`NS`] runs it.
pub const HERMES: &str = "/var/lib/lampos-hermes/.local/bin/hermes";
/// The `hermes` Harness finds on `lampos`'s login `PATH`:
/// [`WRAPPER_TEXT`], root's, 0755.
pub const WRAPPER: &str = "/usr/local/bin/hermes";
/// The script [`WRAPPER`] runs inside its first namespace: [`NS_TEXT`],
/// root's, 0755, beside lamp-update's own fixed files.
pub const NS: &str = "/usr/local/lib/lampos/hermes-ns";
/// lamp-cloud's local token for its Hermes forwarder
/// (`lamp_cloud::hermes_llm::TOKEN_FILE` in its state folder, `lampos`'s,
/// 0600). The only secret Hermes is given.
pub const TOKEN: &str = "/var/lib/lampos-cloud/hermes-token";
/// The variable Hermes reads its forwarder key from: Hermes's
/// `providers.lamp.key_env`, as lamp-tasks' `hermes_config::TOKEN_ENV`
/// writes it.
pub const TOKEN_ENV: &str = "LAMP_HERMES_TOKEN";
/// What [`NS`] hides from Hermes with an empty read-only tmpfs: systemd's
/// credentials (lamp-cloud's relay key, `LoadCredential=key`) and
/// lamp-cloud's state folder (the token's own file, and what else
/// lamp-cloud keeps).
pub const HIDDEN: [&str; 2] = ["/run/credentials", "/var/lib/lampos-cloud"];
/// Free space `/var/lib` must hold before an install: Hermes v0.22.2 took
/// 3.0 GB there on lamp-c0a4, with room to spare.
pub const NEEDS_FREE_KB: u64 = 4 * 1024 * 1024;
/// Memory the kernel says is available ([`MEMINFO`]'s `MemAvailable`)
/// before an install: 1 GiB. From lamp-c0a4 on 10 Nov, which had about
/// 2.1 GB available at rest with the voice running; the install's own peak
/// is still to be measured. pip's `python-deps` stage is the heavy one, and
/// a lamp under the floor would push the voice into swap or the OOM killer.
pub const NEEDS_AVAILABLE_KB: u64 = 1024 * 1024;
/// Where the available memory is read, under the lamp's root.
pub const MEMINFO: &str = "/proc/meminfo";
/// The `PATH` the installer and Hermes's setup run with.
pub const INSTALL_PATH: &str = "/usr/bin:/bin";

/// [`WRAPPER`]: reads the token, fails closed without it, and runs [`NS`]
/// in a new user, mount and PID namespace with its own `/proc`
/// (`--kill-child` forks, and killing the wrapper kills Hermes). Hermes
/// shares `lampos`'s uid with lamp-cloud, so with the lamp's `/proc` it
/// could open lamp-cloud's `/proc/<pid>/root` (its own mount view, the
/// credentials there), `environ` and `fd`; in its own PID namespace no
/// lamp process is there. Nothing else of the caller's changes.
pub const WRAPPER_TEXT: &str = r#"#!/bin/sh
# lampOS: Hermes on the lamp (#5978; lampOS/docs/install.md, "Hermes on the lamp"). Written by lamp-update.
# Hermes runs as its caller (lampos, Harness's user), in a user, mount and PID namespace with its own /proc, where
# lamp-cloud's credentials and state are hidden and no lamp process is visible; it is given only lamp-cloud's local
# forwarder token. It is never run any other way.
T=$(cat /var/lib/lampos-cloud/hermes-token 2>/dev/null) && [ -n "$T" ] || {
  echo "hermes: no forwarder token at /var/lib/lampos-cloud/hermes-token; lamp-cloud makes it with LAMP_HERMES_ON_LAMP=on" >&2
  exit 1
}
LAMP_NS_UID=$(id -u) && LAMP_NS_GID=$(id -g) || exit 1
export LAMP_HERMES_TOKEN="$T" HERMES_HOME=/var/lib/lampos-hermes/.hermes LAMP_NS_UID LAMP_NS_GID
unset T
exec /usr/bin/unshare --map-root-user --mount --pid --kill-child --mount-proc --propagation private /usr/local/lib/lampos/hermes-ns "$@"
"#;

/// [`NS`]: only inside a user namespace (refused in the lamp's own, where
/// it would hide lamp-cloud's files from the whole lamp), it mounts an
/// empty read-only tmpfs over each of [`HIDDEN`] (the credentials only
/// when the folder is there), then runs [`HERMES`] as the caller's own ids
/// in a nested user namespace, where those mounts cannot be undone. Any
/// step failing ends it before Hermes runs.
pub const NS_TEXT: &str = r#"#!/bin/sh
# lampOS: Hermes on the lamp, inside the namespace /usr/local/bin/hermes makes (#5978; lampOS/docs/install.md,
# "Hermes on the lamp"). Written by lamp-update. Root here only: it hides lamp-cloud's credentials and state, then
# drops back to the caller's own ids in a nested user namespace, where these mounts are locked.
read -r _ _ count < /proc/self/uid_map || exit 1
[ "$count" != 4294967295 ] || { echo "hermes-ns: only inside the namespace /usr/local/bin/hermes makes" >&2; exit 1; }
[ -n "${LAMP_NS_UID-}" ] && [ -n "${LAMP_NS_GID-}" ] || exit 1
if [ -e /run/credentials ]; then
  mount -t tmpfs -o size=4k,mode=0555 none /run/credentials || exit 1
fi
mount -t tmpfs -o size=4k,mode=0555 none /var/lib/lampos-cloud || exit 1
U=$LAMP_NS_UID G=$LAMP_NS_GID
unset LAMP_NS_UID LAMP_NS_GID
exec /usr/bin/unshare --user --map-user="$U" --map-group="$G" /var/lib/lampos-hermes/.local/bin/hermes "$@"
"#;

/// Hermes's config set after its install, each with `hermes config set`
/// as `lampos` (its own writer: it changes that key alone and keeps every
/// other line, Harness's `hooks:` block among them):
/// - `updates.check false`: no update check of its own, so no offer of a
///   Hermes past the pin (a `hermes update` is only ever run by hand);
/// - `plugins.auto_update_check_hours 0`: no plugin update check either;
/// - `lsp.enabled false`: no language servers, which Hermes otherwise
///   starts after an edit and keeps for 600 s idle;
/// - `terminal.oneshot_completion_wait_seconds 0`: a `chat -q` run ends
///   with its turn, not up to 600 s later.
///
/// The messaging gateway is never installed or started: its `gateway`
/// stage is not run, and nothing runs `hermes gateway`.
pub const CONFIG: [(&str, &str); 4] = [
    ("updates.check", "false"),
    ("plugins.auto_update_check_hours", "0"),
    ("lsp.enabled", "false"),
    ("terminal.oneshot_completion_wait_seconds", "0"),
];

/// The installer's stages run, in order: version 1's list (the autonomous
/// repo's `runtimes/hermes/install.sh`) as this installer names them, as
/// run on lamp-c0a4. Its `products` stage publishes `hermes` in
/// `~/.local/bin`; `setup` and `gateway` need a person and are left out,
/// as is `complete`, which writes only a marker. No stage installs Node's
/// tools (version 1's `node-deps` hung on ARM), a browser or the
/// computer-use driver (`--skip-browser`, `--skip-computer-use`).
pub const STAGES: [&str; 6] = [
    "prerequisites",
    "repository",
    "venv",
    "python-deps",
    "config",
    "products",
];

/// The hermes-agent commit installed: tag `v0.22.2`, on `main`. Harness
/// before openharness main's fix takes no session id of Hermes 0.22.3 or
/// later.
pub const COMMIT: &str = "d923a5eb487a17c0a68801bba9ef0f56c81040a8";
/// [`COMMIT`]'s release.
pub const VERSION: &str = "0.22.2";
/// The installer, a byte-for-byte copy of [`INSTALLER_ORIGIN`]: the same
/// bytes as `scripts/install.sh` at [`COMMIT`]. Every install runs this
/// copy, from a file, never one fetched.
pub const INSTALLER: &[u8] = include_bytes!("../../../deploy/hermes/install-v0.22.2-f4cdb0e2.sh");
/// [`INSTALLER`]'s SHA-256, checked by a test and again before each run.
pub const INSTALLER_SHA256: &str =
    "f4cdb0e20f1aca2c3871a76b962fd4db61510f549cf973759c753ac13349beb5";
/// Where [`INSTALLER`] was copied from: said, never fetched.
pub const INSTALLER_ORIGIN: &str =
    "https://github.com/NousResearch/hermes-agent/blob/v0.22.2/scripts/install.sh";
/// The file name the checked copy is written as, in a folder of its own.
pub const INSTALLER_FILE: &str = "hermes-install.sh";

/// Set: `on` exactly, any other value off. Unset: `default` on a lamp
/// installed as the product ([`harness_install::product::product_lamp`]),
/// off on any other.
pub fn wanted(switch: Option<&str>, product: bool, default: bool) -> bool {
    match switch {
        Some(value) => value == "on",
        None => product && default,
    }
}

/// Where Hermes's install is, as [`RECORD`] keeps it: `install` one of
/// [`HermesInstall::INSTALL`], `error` (with `failed` only) one of
/// [`HermesInstall::ERRORS`]; `revision` up by one at each write;
/// `version` and `commit` with `installed` only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesInstall {
    pub install: String,
    pub revision: u64,
    pub at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HermesInstall {
    /// Missing and not tried yet (or waiting for Harness); being put there;
    /// there; the last try failed.
    pub const INSTALL: [&str; 4] = ["absent", "installing", "installed", "failed"];
    /// Why the last try failed: the installer's bytes not the pinned ones,
    /// no `lampos` user, under [`NEEDS_FREE_KB`] free in `/var/lib`
    /// (nothing changed then), under [`NEEDS_AVAILABLE_KB`] of memory
    /// available or it not read (nothing changed either), no `git`, `curl`, `bash` or `unshare` on the
    /// lamp (nothing is added to the system for them), no user namespace
    /// for `lampos` (nothing installed), one of lamp-update's own steps
    /// (the installer's file, moving an old Hermes aside, making
    /// [`PLACE`]), past a stage's bound, a stage failed or left no `hermes`
    /// or a `hermes` that does not run, the checkout not at [`COMMIT`],
    /// Hermes's config not set, the wrapper or its namespace script not
    /// written, the one-shot not started.
    pub const ERRORS: [&str; 13] = [
        "pin_mismatch",
        "no_user",
        "no_space",
        "no_memory",
        "no_tools",
        "no_userns",
        "serve",
        "timeout",
        "installer",
        "version",
        "config",
        "entry",
        "not_started",
    ];

    fn new(install: &str, error: Option<&str>, at_unix_ms: u64) -> Self {
        Self {
            install: install.to_owned(),
            revision: 0,
            at_unix_ms,
            version: None,
            commit: None,
            error: error.map(str::to_owned),
        }
    }

    fn installed(at_unix_ms: u64) -> Self {
        Self {
            version: Some(VERSION.to_owned()),
            commit: Some(COMMIT.to_owned()),
            ..Self::new("installed", None, at_unix_ms)
        }
    }

    /// Whether every field is one lamp-update could have written.
    pub fn check(&self) -> bool {
        let plain = |text: &Option<String>| {
            text.as_ref().is_none_or(|text| {
                self.install == "installed"
                    && !text.is_empty()
                    && text.len() <= 64
                    && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
            })
        };
        Self::INSTALL.contains(&self.install.as_str())
            && match &self.error {
                None => self.install != "failed",
                Some(error) => self.install == "failed" && Self::ERRORS.contains(&error.as_str()),
            }
            && plain(&self.version)
            && plain(&self.commit)
    }
}

/// What the round hands the one-shot in [`ASK`].
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Ask {
    version: u32,
    /// Take the entry points out (the switch off), not install.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    remove: bool,
}

/// The install as [`RECORD`] has it, when lamp-update could have written
/// it; else none.
pub fn record(records: &Path) -> Option<HermesInstall> {
    let bytes = fs::read(records.join(RECORD)).ok()?;
    serde_json::from_slice::<HermesInstall>(&bytes)
        .ok()
        .filter(HermesInstall::check)
}

/// `said` written as [`RECORD`], its `revision` one past the one before.
fn write(records: &Path, mut said: HermesInstall) -> HermesInstall {
    said.revision = record(records).map_or(1, |before| before.revision.saturating_add(1));
    if let Ok(text) = serde_json::to_vec(&said) {
        let _ = write_whole(&records.join(RECORD), &text, 0o600);
    }
    said
}

fn failed(at_unix_ms: u64, error: &str) -> HermesInstall {
    HermesInstall::new("failed", Some(error), at_unix_ms)
}

fn on(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

/// The entry points `lampos` reaches Hermes by, any of them there.
fn entry_points(root: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    [WRAPPER, NS]
        .into_iter()
        .map(|path| on(root, path))
        .filter(|path| fs::symlink_metadata(path).is_ok())
}

/// The checkout's commit under `root`, read from its `.git/HEAD` (the
/// installer leaves it detached, so the file holds the commit itself); none
/// when it is not there, on a branch, or not 40 hex digits.
fn head(root: &Path) -> Option<String> {
    let text = fs::read_to_string(on(root, &format!("{CHECKOUT}/.git/HEAD"))).ok()?;
    let commit = text.trim();
    (commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| commit.to_ascii_lowercase())
}

/// Whether the file at `path` under `root` is a regular file holding
/// `text`, not followed through a link.
fn holds(root: &Path, path: &str, text: &str) -> bool {
    let place = on(root, path);
    fs::symlink_metadata(&place).is_ok_and(|meta| meta.is_file())
        && fs::read(&place).is_ok_and(|bytes| bytes == text.as_bytes())
}

/// Hermes there as this build puts it, under `root`: its command, its
/// checkout at [`COMMIT`], and the wrapper and namespace script with this
/// build's text. Any of them otherwise is Hermes missing, put there again.
pub fn present(root: &Path) -> bool {
    fs::metadata(on(root, HERMES)).is_ok()
        && head(root).as_deref() == Some(COMMIT)
        && holds(root, WRAPPER, WRAPPER_TEXT)
        && holds(root, NS, NS_TEXT)
}

/// When `serve` starts: off, an earlier record and ask are taken away; on
/// with Hermes missing and no record, `absent`.
pub fn at_start(root: &Path, records: &Path, switch: bool, now_unix_ms: u64) {
    if !switch {
        for name in [RECORD, ASK] {
            let _ = fs::remove_file(records.join(name));
        }
        return;
    }
    if !present(root) && record(records).is_none() {
        write(records, HermesInstall::new("absent", None, now_unix_ms));
    }
}

fn ask(records: &Path, remove: bool) -> io::Result<()> {
    let text = serde_json::to_vec(&Ask { version: 1, remove }).map_err(io::Error::other)?;
    write_whole(&records.join(ASK), &text, 0o600)
}

/// After a round that reached the channel, with the switch on, and after
/// Harness's own [`harness_install::after_round`]: Hermes there is recorded
/// `installed` (once) and nothing runs. Missing, with no try under way and
/// no recent failure, [`ASK`] is written; then with Harness there,
/// `installing` is recorded and Harness's one-shot started without
/// waiting (a unit not started is `not_started`), and with Harness missing
/// nothing more: the one-shot Harness's round starts reads the ask after
/// Harness. What was recorded is returned, none when nothing changed.
pub fn after_round(
    root: &Path,
    records: &Path,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> Option<HermesInstall> {
    let before = record(records);
    if present(root) {
        if before
            .as_ref()
            .is_some_and(|said| said.install == "installed")
        {
            return None;
        }
        say(&format!(
            "Hermes {VERSION} is at {HERMES}: recorded as installed"
        ));
        return Some(write(records, HermesInstall::installed(now_unix_ms)));
    }
    let since = |said: &HermesInstall| now_unix_ms.saturating_sub(said.at_unix_ms);
    if let Some(said) = &before {
        if said.install == "installing" && since(said) < STUCK_AFTER.as_millis() as u64 {
            return None;
        }
        if said.install == "failed"
            && !matches!(said.error.as_deref(), Some("serve" | "not_started"))
            && since(said) < RETRY_AFTER.as_millis() as u64
        {
            return None;
        }
    }
    if let Err(error) = ask(records, false) {
        say(&format!(
            "Hermes is not installed: {ASK} not written: {error}"
        ));
        return Some(write(records, failed(now_unix_ms, "not_started")));
    }
    if !harness_install::present(root) {
        say("Hermes waits for Harness: Harness's one-shot installs it after Harness");
        return match before {
            Some(said) if said.install == "absent" => None,
            _ => Some(write(
                records,
                HermesInstall::new("absent", None, now_unix_ms),
            )),
        };
    }
    write(records, HermesInstall::new("installing", None, now_unix_ms));
    match harness_install::start(root, systemd) {
        Ok(()) => {
            say(&format!(
                "Hermes {VERSION} is not set up at {HERMES}: {} started to put it there",
                harness_install::UNIT
            ));
            record(records)
        }
        Err(why) => {
            say(&format!(
                "Hermes is not installed: {} not started: {why}",
                harness_install::UNIT
            ));
            Some(write(records, failed(now_unix_ms, "not_started")))
        }
    }
}

/// After a round that reached the channel, with the switch off: an entry
/// point still there (the wrapper or the namespace script) is taken out by
/// Harness's one-shot, asked with [`Ask::remove`] and started without
/// waiting; the round's own sandbox cannot write them. Nothing there,
/// nothing runs. Whether the one-shot was started is returned.
pub fn after_round_off(
    root: &Path,
    records: &Path,
    systemd: &mut dyn Systemd,
    say: &mut dyn FnMut(&str),
) -> bool {
    if entry_points(root).next().is_none() {
        return false;
    }
    let started = ask(records, true)
        .map_err(|error| error.to_string())
        .and_then(|()| harness_install::start(root, systemd));
    match started {
        Ok(()) => {
            say(&format!(
                "Hermes on the lamp is off: {} started to take its wrapper out",
                harness_install::UNIT
            ));
            true
        }
        Err(why) => {
            say(&format!(
                "Hermes on the lamp is off, but its wrapper stays: {why}"
            ));
            false
        }
    }
}

/// `lamp-update harness-install` (Harness's one-shot, as root): Harness's
/// install ([`harness_install::install_now`]) when Harness is missing and
/// a round asked for it, then Hermes's ([`install_now`]) when a round
/// asked for it, whatever Harness's did. Started for Hermes alone, with
/// Harness there or its switch off, Harness's record is left as it is. An
/// error only for Harness: a Hermes failure is said and recorded, never
/// the unit's.
pub fn one_shot(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let harness = (!harness_install::present(root) && records.join(harness_install::ASK).exists())
        .then(|| harness_install::install_now(root, records, run, now_unix_ms, say));
    match install_now(root, records, run, now_unix_ms, say) {
        Ok(Some(said)) => {
            if let Some(error) = said.error {
                say(&format!("Hermes is not installed: {error}"));
            }
        }
        Ok(None) => {}
        Err(why) => say(&format!("Hermes is not installed: {why}")),
    }
    match harness.transpose()?.and_then(|said| said.error) {
        None => Ok(()),
        Some(error) => Err(format!("Harness is not installed: {error}")),
    }
}

/// What an install runs: this build's own installer; a test's may hold
/// other bytes.
struct Kit {
    installer: &'static [u8],
    installer_sha256: &'static str,
    commit: &'static str,
}

impl Kit {
    const BUILT: Kit = Kit {
        installer: INSTALLER,
        installer_sha256: INSTALLER_SHA256,
        commit: COMMIT,
    };
}

/// `lamp-update harness-install`'s Hermes part, after Harness's, run by
/// Harness's one-shot as root: none when no [`ASK`] is there. Asked to
/// remove, the entry points are taken out and the record with them.
/// Else Hermes already there is recorded `installed` and nothing runs;
/// missing, `installing` is recorded and [`steps`] run. A failure takes
/// the entry points out. Each command goes through `run`. The record
/// written is returned; an error only when another install holds
/// [`LOCK`], with nothing written.
pub fn install_now(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
) -> Result<Option<HermesInstall>, String> {
    install_with(root, records, run, now_unix_ms, say, &Kit::BUILT)
}

fn install_with(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
    kit: &Kit,
) -> Result<Option<HermesInstall>, String> {
    let Ok(bytes) = fs::read(records.join(ASK)) else {
        return Ok(None);
    };
    let asked = serde_json::from_slice::<Ask>(&bytes).unwrap_or_default();
    let _lock = lock(records)?;
    let said = if asked.remove {
        remove_entry_points(root, say);
        let _ = fs::remove_file(records.join(RECORD));
        None
    } else {
        let said = steps(root, records, run, now_unix_ms, say, kit);
        if said.install == "failed" {
            remove_entry_points(root, say);
        }
        Some(said)
    };
    // Answered: a round asks again when it wants another try.
    let _ = fs::remove_file(records.join(ASK));
    Ok(said)
}

/// The wrapper and the namespace script taken out, the wrapper first, so
/// `lampos` finds no `hermes`. Hermes's folder and Hermes in it stay.
fn remove_entry_points(root: &Path, say: &mut dyn FnMut(&str)) {
    for path in [WRAPPER, NS] {
        match fs::remove_file(on(root, path)) {
            Ok(()) => say(&format!("{path} taken out")),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => say(&format!("{path} not taken out: {error}")),
        }
    }
}

/// The install, in order: the installer's bytes checked; `lampos` and its
/// ids looked up; `git`, `curl`, `bash` and `unshare` looked for as
/// `lampos`; the namespace tried as `lampos`. Then, unless Hermes is in
/// [`PLACE`] at [`COMMIT`] already (`lampos`'s): the free space checked, a
/// Hermes of another commit moved aside to [`ASIDE`], [`PLACE`] made
/// `lampos`'s 0700, the checked installer written as a file and run as
/// `lampos` stage by stage, its `hermes` looked for and the checkout's
/// commit read. Then Hermes's [`CONFIG`] set, `hermes --version` run, the
/// namespace script and the wrapper written (the wrapper last), [`ASIDE`]
/// removed, and, after a new install, lamp-tasks started again if it runs,
/// so it merges Hermes's settings at its start.
fn steps(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
    kit: &Kit,
) -> HermesInstall {
    if present(root) && kit.commit == COMMIT {
        say(&format!(
            "Hermes {VERSION} is already at {HERMES}: nothing installed"
        ));
        if let Some(said) = record(records)
            && said.install == "installed"
        {
            return said;
        }
        return write(records, HermesInstall::installed(now_unix_ms()));
    }
    write(
        records,
        HermesInstall::new("installing", None, now_unix_ms()),
    );
    let mut fail = |error: &str, why: String, say: &mut dyn FnMut(&str)| {
        say(&format!("Hermes is not installed: {why}"));
        write(records, failed(now_unix_ms(), error))
    };
    let found = crate::manifest::sha256_hex(kit.installer);
    if found != kit.installer_sha256 {
        let why = format!(
            "the pinned installer's sha256 is {found}, not {}",
            kit.installer_sha256
        );
        return fail("pin_mismatch", why, say);
    }
    let ids = install::uid_of(run, USER).and_then(|uid| Ok((uid, gid_of(run, USER)?)));
    let (uid, gid) = match ids {
        Ok(ids) => ids,
        Err(why) => return fail("no_user", why, say),
    };
    for (tool, argv) in tool_checks() {
        if run(&argv).is_err() {
            return fail(
                "no_tools",
                format!("no {tool} on the lamp, which this install needs"),
                say,
            );
        }
    }
    if let Err(why) = run(&userns_argv(&uid, &gid)) {
        let why = format!(
            "no user namespaces for {USER} (unshare --map-root-user --mount, then --user back to {uid}:{gid}): {why}; \
             Hermes would see lamp-cloud's relay key, so nothing is installed"
        );
        return fail("no_userns", why, say);
    }
    let pinned_there = fs::metadata(on(root, HERMES)).is_ok()
        && head(root).as_deref() == Some(kit.commit)
        && owner(root, PLACE).as_deref() == Some(uid.as_str());
    if !pinned_there {
        match available_kb(root) {
            Some(available) if available >= NEEDS_AVAILABLE_KB => {}
            available => {
                let why = match available {
                    Some(available) => format!(
                        "{available} KiB of memory available, under the {NEEDS_AVAILABLE_KB} KiB it needs"
                    ),
                    None => format!("MemAvailable not read from {MEMINFO}"),
                };
                return fail("no_memory", why, say);
            }
        }
        match free_kb(run) {
            Some(free) if free >= NEEDS_FREE_KB => {}
            free => {
                let why = match free {
                    Some(free) => {
                        format!(
                            "{free} KiB free in /var/lib, under the {NEEDS_FREE_KB} KiB it needs"
                        )
                    }
                    None => "the free space in /var/lib not read".to_owned(),
                };
                return fail("no_space", why, say);
            }
        }
        if let Err(why) = set_aside(root, &uid, kit.commit, say) {
            return fail("serve", why, say);
        }
        if let Err(why) =
            run(&["install", "-d", "-o", USER, "-g", USER, "-m", "0700", PLACE].map(str::to_owned))
        {
            return fail("serve", format!("{PLACE} not made {USER}'s: {why}"), say);
        }
        let file = match InstallerFile::write(kit.installer, kit.installer_sha256, say) {
            Ok(file) => file,
            Err(error) => return fail(error, "the installer's file".to_owned(), say),
        };
        let installer = file.path().to_string_lossy().into_owned();
        for stage in STAGES {
            let argv = [
                "bash",
                installer.as_str(),
                "--stage",
                stage,
                "--non-interactive",
                "--skip-browser",
                "--skip-computer-use",
                "--branch",
                "main",
                "--commit",
                kit.commit,
            ];
            let started = std::time::Instant::now();
            if let Err(why) = run(&as_lampos(&argv)) {
                drop(file);
                let error = if install::timed_out(&why) {
                    "timeout"
                } else {
                    "installer"
                };
                return fail(error, format!("its installer's {stage} stage: {why}"), say);
            }
            say(&format!(
                "Hermes's installer: stage {stage} done in {} s",
                started.elapsed().as_secs()
            ));
        }
        drop(file);
        if fs::metadata(on(root, HERMES)).is_err() {
            return fail(
                "installer",
                format!("no hermes at {HERMES} after its installer"),
                say,
            );
        }
        let have = head(root);
        if have.as_deref() != Some(kit.commit) {
            let why = format!(
                "its checkout is at {}, not the pinned {}",
                have.as_deref().unwrap_or("no commit"),
                kit.commit
            );
            return fail("version", why, say);
        }
    }
    for (key, value) in CONFIG {
        if let Err(why) = run(&as_lampos(&[HERMES, "config", "set", key, value])) {
            return fail(
                "config",
                format!("Hermes's {key} not set to {value}: {why}"),
                say,
            );
        }
    }
    match run(&as_lampos(&[HERMES, "--version"])) {
        Ok(said) => say(&format!(
            "Hermes runs: {}",
            said.lines().next().unwrap_or("").trim()
        )),
        Err(why) => {
            return fail(
                "installer",
                format!("the new Hermes does not run: {why}"),
                say,
            );
        }
    }
    if let Err(why) = write_entry_points(root) {
        return fail("entry", why, say);
    }
    remove_aside(root, say);
    if !pinned_there {
        // lamp-tasks merges Hermes's settings (its forwarder, its skills) at
        // its start: started again now if it runs, never started otherwise.
        if let Err(why) =
            run(&["systemctl", "try-restart", "lamp-tasks.service"].map(str::to_owned))
        {
            say(&format!("lamp-tasks.service not started again: {why}"));
        }
    }
    say(&format!(
        "Hermes {VERSION} ({}) installed for {USER} at {HERMES}; Harness runs it through {WRAPPER}, in a namespace that hides lamp-cloud's credentials; no service, nothing at boot",
        kit.commit
    ));
    write(records, HermesInstall::installed(now_unix_ms()))
}

/// `user`'s group id, from `id -g`, through `run`.
fn gid_of(run: &mut Runner, user: &str) -> Result<String, String> {
    let gid = run(&["id".to_owned(), "-g".to_owned(), user.to_owned()])?
        .trim()
        .to_owned();
    if gid.is_empty() || !gid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("id -g {user} gave no group id"));
    }
    Ok(gid)
}

/// The owner's uid of `path` under `root`, not followed through a link.
fn owner(root: &Path, path: &str) -> Option<String> {
    fs::symlink_metadata(on(root, path))
        .ok()
        .filter(|meta| meta.is_dir())
        .map(|meta| meta.uid().to_string())
}

/// A Hermes in [`PLACE`] that is not this build's moved out of the way:
/// its checkout at another commit or on a branch, a `hermes` with no
/// checkout, or the folder another user's. Moved whole to [`ASIDE`]; one
/// there already from an earlier try is kept, and this one removed
/// instead. A link at [`PLACE`] is removed, never followed. Nothing there,
/// or a checkout at the pin that an earlier try left unfinished (the
/// installer repairs it), is left as it is.
fn set_aside(
    root: &Path,
    uid: &str,
    commit: &str,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let place = on(root, PLACE);
    let Ok(meta) = fs::symlink_metadata(&place) else {
        return Ok(());
    };
    if !meta.is_dir() {
        return fs::remove_file(&place).map_err(|error| format!("{PLACE}: {error}"));
    }
    let other = match head(root) {
        Some(have) => have != commit,
        None => fs::symlink_metadata(on(root, HERMES)).is_ok(),
    } || meta.uid().to_string() != uid;
    if !other {
        return Ok(());
    }
    let aside = on(root, ASIDE);
    if fs::symlink_metadata(&aside).is_ok() {
        say(&format!(
            "another Hermes in {PLACE}, and one already aside at {ASIDE}: this one removed"
        ));
        fs::remove_dir_all(&place).map_err(|error| format!("{PLACE} not removed: {error}"))
    } else {
        say(&format!(
            "another Hermes in {PLACE} (checkout at {}): moved aside to {ASIDE}",
            head(root).as_deref().unwrap_or("no commit")
        ));
        fs::rename(&place, &aside).map_err(|error| format!("{PLACE} not moved aside: {error}"))
    }
}

/// [`ASIDE`] removed, once the new Hermes runs: a link as a link.
fn remove_aside(root: &Path, say: &mut dyn FnMut(&str)) {
    let aside = on(root, ASIDE);
    let Ok(meta) = fs::symlink_metadata(&aside) else {
        return;
    };
    let removed = if meta.is_dir() {
        fs::remove_dir_all(&aside)
    } else {
        fs::remove_file(&aside)
    };
    match removed {
        Ok(()) => say(&format!("the Hermes moved aside to {ASIDE} removed")),
        Err(error) => say(&format!("{ASIDE} not removed: {error}")),
    }
}

/// The KiB free in `/var/lib`, where [`PLACE`] goes, from `df -Pk`: its
/// second line's fourth field.
fn free_kb(run: &mut Runner) -> Option<u64> {
    let text = run(&["df", "-Pk", "/var/lib"].map(str::to_owned)).ok()?;
    text.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()
}

/// The KiB of memory available, from [`MEMINFO`]'s `MemAvailable` line;
/// none when the file or the line is not read.
fn available_kb(root: &Path) -> Option<u64> {
    let text = fs::read_to_string(on(root, MEMINFO)).ok()?;
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?;
    let mut words = line.split_whitespace();
    let kb = words.next()?.parse().ok()?;
    (words.next() == Some("kB") && words.next().is_none()).then_some(kb)
}

/// What the install needs on the lamp, each looked for as `lampos` with
/// the install's `PATH`: `git`, `curl` and `bash` for the installer,
/// `unshare` for the wrapper.
fn tool_checks() -> Vec<(&'static str, Vec<String>)> {
    ["git", "curl", "bash"]
        .into_iter()
        .map(|tool| {
            (
                tool,
                as_lampos(&["sh", "-c", &format!("command -v {tool}")]),
            )
        })
        .chain([(
            "unshare",
            vec![
                "test".to_owned(),
                "-x".to_owned(),
                "/usr/bin/unshare".to_owned(),
            ],
        )])
        .collect()
}

/// The namespace the wrapper makes, tried as `lampos` before anything is
/// installed: a user and mount namespace with `lampos` as its root, then a
/// nested user namespace back to `lampos`'s own `uid` and `gid`, running
/// `true`.
pub fn userns_argv(uid: &str, gid: &str) -> Vec<String> {
    [
        "runuser",
        "-u",
        USER,
        "--",
        "/usr/bin/unshare",
        "--map-root-user",
        "--mount",
        "--propagation",
        "private",
        "/usr/bin/unshare",
        "--user",
    ]
    .map(str::to_owned)
    .into_iter()
    .chain([
        format!("--map-user={uid}"),
        format!("--map-group={gid}"),
        "true".to_owned(),
    ])
    .collect()
}

/// The namespace script, then the wrapper: root's, 0755, each written
/// whole beside and renamed in (never through a link), so `lampos` never
/// finds a wrapper before what it runs.
fn write_entry_points(root: &Path) -> Result<(), String> {
    for (path, text) in [(NS, NS_TEXT), (WRAPPER, WRAPPER_TEXT)] {
        let place = on(root, path);
        if let Some(folder) = place.parent()
            && !folder.is_dir()
        {
            return Err(format!("{}: no such folder", folder.display()));
        }
        write_whole(&place, text.as_bytes(), 0o755).map_err(|error| format!("{path}: {error}"))?;
    }
    Ok(())
}

/// `args` as `lampos`, with [`PLACE`] as `HOME` (so the installer's
/// `~/.local` and its shell lines go there), Hermes's home, the install's
/// `PATH` and `SHELL=/bin/sh`.
fn as_lampos(args: &[&str]) -> Vec<String> {
    let mut argv: Vec<String> = ["runuser", "-u", USER, "--", "env"]
        .map(str::to_owned)
        .into();
    argv.extend([
        format!("HOME={PLACE}"),
        format!("HERMES_HOME={HERMES_HOME}"),
        format!("PATH={INSTALL_PATH}"),
        "SHELL=/bin/sh".to_owned(),
    ]);
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    argv
}

/// The checked installer written as a file for `bash` to run: in a folder
/// of its own under the one-shot's private `/tmp`, root's, the folder 0755
/// and the file 0644, so `lampos` reads it but cannot change it. Taken
/// away when dropped.
struct InstallerFile {
    folder: PathBuf,
}

impl InstallerFile {
    /// `bytes` written and read back against `sha256`: a write that cannot
    /// be made is `serve`, other bytes read back `pin_mismatch`.
    fn write(bytes: &[u8], sha256: &str, say: &mut dyn FnMut(&str)) -> Result<Self, &'static str> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let folder =
            std::env::temp_dir().join(format!("lamp-update-hermes-{}-{stamp}", std::process::id()));
        let written = Self {
            folder: folder.clone(),
        };
        let made = fs::DirBuilder::new()
            .mode(0o755)
            .create(&folder)
            .and_then(|()| fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)))
            .and_then(|()| {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o644)
                    .open(written.path())?;
                file.write_all(bytes)?;
                file.sync_all()?;
                fs::set_permissions(written.path(), fs::Permissions::from_mode(0o644))
            });
        if let Err(error) = made {
            say(&format!(
                "the pinned Hermes installer was not written at {}: {error}",
                folder.display()
            ));
            return Err("serve");
        }
        let found = fs::read(written.path()).map(|read| crate::manifest::sha256_hex(&read));
        if found.as_deref().ok() != Some(sha256) {
            say(&format!(
                "the pinned Hermes installer written at {} is not sha256 {sha256}: nothing installed",
                folder.display()
            ));
            return Err("pin_mismatch");
        }
        Ok(written)
    }

    fn path(&self) -> PathBuf {
        self.folder.join(INSTALLER_FILE)
    }
}

impl Drop for InstallerFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

/// [`LOCK`], held while this install runs; one held already is refused.
fn lock(records: &Path) -> Result<fs::File, String> {
    fs::create_dir_all(records).map_err(|error| format!("{}: {error}", records.display()))?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(records.join(LOCK))
        .map_err(|error| format!("{LOCK}: {error}"))?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
        |_| io::Error::new(ErrorKind::WouldBlock, "another Hermes install is at work").to_string(),
    )?;
    Ok(file)
}

#[cfg(test)]
mod tests;
