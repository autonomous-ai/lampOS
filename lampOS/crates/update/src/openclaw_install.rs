//! OpenClaw put on the lamp beside Hermes (#6226; docs/install.md,
//! "OpenClaw on the lamp"), in the same role and under the same rules as
//! [`crate::hermes_install`]: on by default on a lamp installed as the
//! product ([`PRODUCT_DEFAULT`]); `LAMP_OPENCLAW_ON_LAMP=off` in
//! lamp-update's own unit turns it off, and any other lamp has it only with
//! `on` there. It rides on Harness before the sign-in: with that off,
//! OpenClaw is off as well.
//!
//! Harness finds OpenClaw as it finds Hermes, by `command -v openclaw` in
//! `lampos`'s login shell: [`WRAPPER`], in `/usr/local/bin`. Unlike Hermes,
//! OpenClaw needs a running gateway (loopback port [`PORT`], token auth)
//! that every Harness pane talks to; Harness never starts it. So this
//! install also leaves [`UNIT`], which runs `openclaw gateway run` through
//! the same wrapper as `lampos`. The gateway is always on while the switch
//! is (the owner on #6226: every lamp has 6 GB): the round enables and
//! starts it once OpenClaw is there, an install restarts it, and the
//! switch off disables and stops it. The install leaves the gateway's token ([`GATEWAY_TOKEN`])
//! in OpenClaw's first config ([`CONFIG`], its `gateway` block only:
//! lamp-tasks merges its models, agents and skills keys in at its start).
//!
//! **As `lampos`, in a namespace, as Hermes.** [`WRAPPER`] and [`NS`] are
//! Hermes's ([`crate::hermes_install::WRAPPER_TEXT`],
//! [`crate::hermes_install::ns_text`]) with OpenClaw's folder and command:
//! OpenClaw sees only [`ALLOWED`] (its own folder, the work folder, the
//! skills read-only) over the same empty read-only `/run`, `/var/lib` and
//! `/etc/lampOS`, its own `/tmp`, and no camera, mic or serial device. The
//! gateway runs the tools the model chooses, so it is the gateway that runs
//! in the namespace. Unlike Hermes, OpenClaw shares the lamp's PID
//! namespace ([`NS_FLAGS`]: a user and mount namespace only). OpenClaw
//! 2026.11.2 refuses to open its state from a PID namespace other than its
//! Gateway's (`GatewayLockNamespaceError`), so with one each, every Harness
//! pane exited at once on lamp-c0a4 and no task was delivered. So OpenClaw
//! sees the lamp's process list (names and command lines in `/proc`). It
//! still reaches no file, socket or device beyond what its mount namespace
//! shows: another user namespace's processes' `/proc/<pid>/root`, `cwd`,
//! `fd` and `environ` are refused to it (the kernel's ptrace check). But it
//! runs as `lampos`, the user every lamp service runs as, so it can send
//! them signals; its own PID namespace prevented that, and a namespace
//! shared by the Gateway and its panes is the follow-up that would again. It is given only lamp-cloud's
//! local forwarder token ([`TOKEN`], as [`TOKEN_ENV`]: the forwarder is the
//! one Hermes uses). With [`SKILLS_SWITCH`] on, the namespace also shows
//! lamp-skills' socket folder, read-only, as Hermes's does with its own
//! switch.
//!
//! **When.** As Hermes's: after a `serve` round that reached the channel,
//! in the window of [`crate::hermes_schedule::window`], by Harness's
//! one-shot, watched by the same [`crate::hermes_schedule::Watcher`]. The
//! one-shot installs OpenClaw after Hermes; with both missing, the two
//! installs take turns: one under way holds the other's ask to a later
//! round ([`under_way`]), so the one-shot is never left past an ask.
//!
//! **Pinned.** A Node 24 runtime ([`NODE_VERSION`], nodejs.org's
//! linux-arm64 tarball from the same place as Harness's pin, checked by its
//! SHA-256 [`NODE_SHA256`], never fetched loose) is unpacked
//! at [`NODE_DIR`]. The npm package `openclaw` [`VERSION`] and its whole
//! tree are installed by `npm ci` from a lockfile shipped in this build
//! ([`LOCKFILE`], every package with its sha512 integrity, the file itself
//! checked by its SHA-256 [`LOCKFILE_SHA256`], its `openclaw` entry
//! checked against [`PACKAGE_URL`] and [`PACKAGE_INTEGRITY`]), never
//! resolved freely. npm runs as `lampos` inside OpenClaw's own namespace
//! ([`NPM_CI_ENV`], the one mode of [`NS`]), with no package's install
//! scripts: OpenClaw finishes its own package step at its first run (the
//! install's `openclaw --version`, in the namespace too).
use crate::{
    harness_install,
    hermes_install::{
        self, NEEDS_AVAILABLE_KB, RETRY_IN_NEXT_WINDOW, available_kb, free_kb, gid_of, holds,
        lock_named, on, owner, userns_argv,
    },
    install::{self, Runner, USER},
    records::write_whole,
    world::Systemd,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, ErrorKind},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The switch, in lamp-update's own unit: `on` exactly.
pub const SWITCH: &str = "LAMP_OPENCLAW_ON_LAMP";
/// Whether a lamp installed as the product installs OpenClaw with
/// [`SWITCH`] unset: on, the lamp's default engine under Harness (#6226),
/// under the same schedule as Hermes; `off` turns it off. Any other lamp
/// with it unset is off.
pub const PRODUCT_DEFAULT: bool = true;
/// The skills switch, in lamp-update's own unit: `on` exactly shows
/// OpenClaw [`SKILLS_SOCKET`] ([`ns_text`]); read as [`SWITCH`] is, with
/// [`SKILLS_PRODUCT_DEFAULT`].
pub const SKILLS_SWITCH: &str = "LAMP_OPENCLAW_SKILLS";
/// On by default on a product lamp, as Hermes's skills switch is.
pub const SKILLS_PRODUCT_DEFAULT: bool = true;

/// Where the install is, in lamp-update's records (0600).
pub const RECORD: &str = "openclaw-install.json";
/// What the round hands the one-shot: install OpenClaw, or take its entry
/// points out.
pub const ASK: &str = "openclaw-ask.json";
/// One OpenClaw install at a time.
pub const LOCK: &str = "openclaw.lock";
/// As Hermes's: an `installing` older than this is taken as ended.
pub const STUCK_AFTER: Duration = hermes_install::STUCK_AFTER;
/// As Hermes's: a failure the same steps would likely repeat waits this.
pub const RETRY_AFTER: Duration = hermes_install::RETRY_AFTER;

/// OpenClaw's folder: `lampos`'s, 0700, the `HOME` it runs with. Its Node,
/// its package, its state and the gateway's token are all under it.
pub const PLACE: &str = "/var/lib/lampos-openclaw";
/// Where the Node and the package of an OpenClaw not this build's are
/// moved before an install (its state stays), removed once the new one
/// runs.
pub const ASIDE: &str = "/var/lib/lampos-openclaw.old";
/// OpenClaw's state (`OPENCLAW_STATE_DIR`): its config, its sessions, its
/// workspace.
pub const STATE: &str = "/var/lib/lampos-openclaw/state";
/// The variable OpenClaw reads its state folder from.
pub const STATE_ENV: &str = "OPENCLAW_STATE_DIR";
/// OpenClaw's config, in [`STATE`]: written here with its `gateway` block
/// only; lamp-tasks merges in the rest.
pub const CONFIG: &str = "/var/lib/lampos-openclaw/state/openclaw.json";
/// The gateway's token, made once at the install: `lampos`'s, 0600.
pub const GATEWAY_TOKEN: &str = "/var/lib/lampos-openclaw/gateway-token";
/// The gateway's loopback port.
pub const PORT: u16 = 18789;
/// The pinned Node runtime, unpacked whole.
pub const NODE_DIR: &str = "/var/lib/lampos-openclaw/node";
/// Its `bin`, first on the `PATH` OpenClaw runs with.
pub const NODE_BIN: &str = "/var/lib/lampos-openclaw/node/bin";
/// Its `node`.
pub const NODE: &str = "/var/lib/lampos-openclaw/node/bin/node";
/// Its `npm`.
pub const NPM: &str = "/var/lib/lampos-openclaw/node/bin/npm";
/// The header the Node version is read from, never run.
pub const NODE_VERSION_H: &str = "/var/lib/lampos-openclaw/node/include/node/node_version.h";
/// The npm folder OpenClaw is installed in: [`MANIFEST`] and [`LOCKFILE`]
/// written there by root, then `npm ci` run there.
pub const PREFIX: &str = "/var/lib/lampos-openclaw/npm";
/// [`MANIFEST`] there.
pub const MANIFEST_AT: &str = "/var/lib/lampos-openclaw/npm/package.json";
/// [`LOCKFILE`] there.
pub const LOCKFILE_AT: &str = "/var/lib/lampos-openclaw/npm/package-lock.json";
/// The `openclaw` command npm links there. Never on any `PATH`: only
/// [`NS`] runs it.
pub const OPENCLAW: &str = "/var/lib/lampos-openclaw/npm/node_modules/.bin/openclaw";
/// The installed package's own `package.json`, its version read there.
pub const PACKAGE_JSON: &str = "/var/lib/lampos-openclaw/npm/node_modules/openclaw/package.json";
/// The `openclaw` Harness finds on `lampos`'s login `PATH`:
/// [`WRAPPER_TEXT`], root's, 0755.
pub const WRAPPER: &str = "/usr/local/bin/openclaw";
/// The script [`WRAPPER`] runs inside its first namespace: [`ns_text`],
/// root's, 0755.
pub const NS: &str = "/usr/local/lib/lampos/openclaw-ns";
/// The gateway's unit: written by the round ([`UNIT_TEXT`]), enabled
/// ([`WANTS`]) and started by it once OpenClaw is there, restarted by an
/// install; disabled and stopped by the round with the switch off.
pub const UNIT: &str = "lamp-openclaw-gateway.service";
/// lamp-cloud's forwarder token, the one Hermes is given.
pub const TOKEN: &str = hermes_install::TOKEN;
/// The variable the forwarder's key is given in: the same as Hermes's, so
/// lamp-tasks' provider entry names `${LAMP_HERMES_TOKEN}`.
pub const TOKEN_ENV: &str = hermes_install::TOKEN_ENV;
/// Harness's project, where it starts OpenClaw's panes: as Hermes's.
pub const WORK: &str = hermes_install::WORK;
/// The lamp's skills, shown read-only.
pub const SKILLS: &str = hermes_install::SKILLS;
/// lamp-skills' socket folder, shown read-only with [`SKILLS_SWITCH`] on.
pub const SKILLS_SOCKET: &str = hermes_install::SKILLS_SOCKET;
/// All [`NS`] shows OpenClaw of what it hides, as Hermes's does.
pub const ALLOWED: [&str; 3] = [PLACE, WORK, SKILLS];
/// Free space `/var/lib` must hold before an install: the package unpacks
/// to 275 MB, its dependencies and npm's cache come on top, and Node is
/// about 190 MB. Not measured on a lamp yet: 2 GiB with room to spare.
pub const NEEDS_FREE_KB: u64 = 2 * 1024 * 1024;
/// The `PATH` npm and OpenClaw run with: the pinned Node first.
pub const INSTALL_PATH: &str = "/var/lib/lampos-openclaw/node/bin:/usr/bin:/bin";

/// The pinned Node. TODO(#6226): share one pinned Node 24 with Harness's
/// (crates/update/src/harness_install) once both have landed.
pub const NODE_VERSION: &str = "24.22.2";
/// Where it is fetched: the same tarball Harness's Node pin fetches, on
/// lampOS's own storage (58030966 bytes, root `node-v24.22.2-linux-arm64`),
/// byte-for-byte nodejs.org's ([`NODE_ORIGIN`]). Only its path is shared
/// with Harness: Harness's own Node is under lampos-tasks' 0700 home.
pub const NODE_URL: &str = "https://storage.googleapis.com/s3-autonomous-upgrade-3/harness/runtime/node/v24.22.2/node-v24.22.2-linux-arm64.tar.gz";
/// Where [`NODE_URL`]'s bytes come from: said, never fetched.
pub const NODE_ORIGIN: &str = "https://nodejs.org/dist/v24.22.2/node-v24.22.2-linux-arm64.tar.gz";
/// Its SHA-256, from nodejs.org's `SHASUMS256.txt` for v24.22.2 (the
/// same as Harness's pin).
pub const NODE_SHA256: &str = "95c264150aceb7f3437a29fd6984924f5f8d1511ea3cbd6c508b83f6c555dae2";
/// The OpenClaw package installed: the one Harness measured.
pub const VERSION: &str = "2026.11.2";
/// Its tarball, as [`LOCKFILE`]'s `openclaw` entry must give it.
pub const PACKAGE_URL: &str = "https://registry.npmjs.org/openclaw/-/openclaw-2026.11.2.tgz";
/// Its integrity, as the registry gives it (`dist.integrity`): what
/// [`LOCKFILE`]'s `openclaw` entry must hold, so npm checks the tarball by it.
pub const PACKAGE_INTEGRITY: &str = "sha512-SgBml4WLp8V1DdAYkrTPLzVdA/iYo3iuHCtm4788GjaG83QZdd1VeOTlue0HBJGdn3SbFE06b6uPs+AM5ShMgg==";
/// The npm project `npm ci` installs: `openclaw` at [`VERSION`] exactly.
pub const MANIFEST: &str = include_str!("../openclaw/package.json");
/// Its lockfile, made once on a developer's machine by `npm install
/// --package-lock-only --ignore-scripts` (npm 11, lockfile version 3) for
/// [`MANIFEST`]: every package of the tree with its registry tarball and
/// sha512 integrity, so npm installs that tree and no other. A new
/// [`VERSION`] makes a new lockfile, and a new [`LOCKFILE_SHA256`].
pub const LOCKFILE: &str = include_str!("../openclaw/package-lock.json");
/// [`LOCKFILE`]'s SHA-256, checked by a test and again before each install.
pub const LOCKFILE_SHA256: &str =
    "2a916259319f5f67b1f8a93d3431839a81ea0f233ceb4cbc18676ecb72962fee";
/// Set to `on` for [`NS`], it runs `npm ci` in [`PREFIX`] instead of
/// OpenClaw: lamp-update's install step, in OpenClaw's namespace.
pub const NPM_CI_ENV: &str = "LAMP_OPENCLAW_NPM_CI";
/// The flags of the first `unshare` (the wrapper's and the install's): a
/// user and mount namespace, no PID namespace (see the module's notes).
pub const NS_FLAGS: [&str; 4] = ["--map-root-user", "--mount", "--propagation", "private"];
/// The hard bound of each of OpenClaw's long steps ([`runs_long_step`]:
/// a fetch, Node's unpacking, npm's install), past which the step is
/// killed and the install recorded `failed` with `timeout` (#6226: on
/// lamp-c0a4 an npm install with no output flags hung 38 min at its
/// extract; with `--foreground-scripts --loglevel=http` it took 3 min).
/// The clock goes on while the watch has the one-shot frozen.
pub const STEP_WITHIN: Duration = Duration::from_secs(20 * 60);
/// The largest file fetched (`curl --max-filesize`).
const MOST_BYTES: &str = "536870912";
/// The prefix of the folder fetched files are kept in while the install
/// runs: a step that names it is a long one ([`runs_long_step`]).
const DOWNLOADS: &str = "lamp-update-openclaw-";

/// [`WRAPPER`]: Hermes's wrapper with OpenClaw's state folder and the
/// pinned Node first on the `PATH`. Reads the token, fails closed without
/// it, and runs [`NS`] in a new user and mount namespace ([`NS_FLAGS`]),
/// in the lamp's PID namespace, which every OpenClaw process shares with
/// its Gateway.
pub const WRAPPER_TEXT: &str = r#"#!/bin/sh
# lampOS: OpenClaw on the lamp (#6226; lampOS/docs/install.md, "OpenClaw on the lamp"). Written by lamp-update.
# OpenClaw (its gateway and each Harness pane) runs as its caller (lampos), in a user and mount namespace where it sees
# only its own folder, its work folder and the skills, no lamp service's files or sockets and no camera, mic or serial
# device; it is given only lamp-cloud's local forwarder token. It shares the lamp's PID namespace (OpenClaw opens its
# state only from its Gateway's): it sees the lamp's process list and, as lampos, can signal lampos's processes, but
# reads none of their files or environment. It is never run any other way.
T=$(cat /var/lib/lampos-cloud/hermes-token 2>/dev/null) && [ -n "$T" ] || {
  echo "openclaw: no forwarder token at /var/lib/lampos-cloud/hermes-token; lamp-cloud makes it with LAMP_OPENCLAW_ON_LAMP=on" >&2
  exit 1
}
LAMP_NS_UID=$(id -u) && LAMP_NS_GID=$(id -g) || exit 1
export LAMP_HERMES_TOKEN="$T" OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state PATH="/var/lib/lampos-openclaw/node/bin:$PATH" LAMP_NS_UID LAMP_NS_GID
unset T
exec /usr/bin/unshare --map-root-user --mount --propagation private /usr/local/lib/lampos/openclaw-ns "$@"
"#;

/// What [`ns_text`] changes in Hermes's namespace script, in order, each
/// everywhere it is: OpenClaw's command, its issue and section, its
/// wrapper, its script's name, its folder, then its name. A test holds
/// that each is found and that no Hermes is left.
const ENGINE_EDITS: [(&str, &str); 7] = [
    (
        "/var/lib/lampos-hermes/.local/bin/hermes \"$@\"",
        "/var/lib/lampos-openclaw/npm/node_modules/.bin/openclaw \"$@\"",
    ),
    (
        "(#5978; lampOS/docs/install.md,\n# \"Hermes on the lamp\")",
        "(#6226; lampOS/docs/install.md,\n# \"OpenClaw on the lamp\")",
    ),
    (
        "LAMP_HERMES_SKILLS=on (#6128)",
        "LAMP_OPENCLAW_SKILLS=on (#6226)",
    ),
    ("/usr/local/bin/hermes", "/usr/local/bin/openclaw"),
    ("hermes-ns: ", "openclaw-ns: "),
    ("H=/var/lib/lampos-hermes ", "H=/var/lib/lampos-openclaw "),
    ("Hermes", "OpenClaw"),
];

/// The line [`NPM_CI_MODE`] goes before: the script's last, which runs
/// OpenClaw.
const RUN_LINE: &str = "exec /usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" /var/lib/lampos-openclaw/npm/node_modules/.bin/openclaw \"$@\"\n";

/// [`NS`]'s one other mode, before OpenClaw would run: with [`NPM_CI_ENV`]
/// `on`, the pinned Node's npm runs `npm ci` in [`PREFIX`] from the
/// lockfile root put there, with no install scripts, as the caller's own
/// ids, behind the same mounts; its arguments are not used. The variable
/// is taken out of OpenClaw's environment either way.
const NPM_CI_MODE: &str = "M=${LAMP_OPENCLAW_NPM_CI-}
unset LAMP_OPENCLAW_NPM_CI
# LAMP_OPENCLAW_NPM_CI=on: lamp-update's install step. npm ci from the pinned lockfile lamp-update wrote in OpenClaw's npm
# folder, with no package's install scripts, behind these same mounts; nothing else.
if [ \"$M\" = on ]; then
  cd /var/lib/lampos-openclaw/npm || fail \"no /var/lib/lampos-openclaw/npm\"
  exec /usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" /var/lib/lampos-openclaw/node/bin/node /var/lib/lampos-openclaw/node/bin/npm ci --omit=dev --ignore-scripts --no-audit --no-fund --no-update-notifier --loglevel=http
fi
";

/// [`NS`]'s text: Hermes's ([`hermes_install::ns_text`]`(skills)`, the
/// same allow-list, the same steps) for OpenClaw ([`ENGINE_EDITS`]), with
/// its install mode ([`NPM_CI_MODE`]) before OpenClaw runs. With
/// `skills`, lamp-skills' socket folder is shown read-only too.
pub fn ns_text(skills: bool) -> String {
    let mut text = hermes_install::ns_text(skills);
    for (from, to) in ENGINE_EDITS {
        text = text.replace(from, to);
    }
    text.replacen(RUN_LINE, &format!("{NPM_CI_MODE}{RUN_LINE}"), 1)
}

/// [`UNIT`], written by a round (whose sandbox writes
/// `/etc/systemd/system`; Harness's one-shot's cannot). The gateway runs
/// through [`WRAPPER`] as `lampos`, so in OpenClaw's namespace, with
/// `HOME` [`PLACE`] and [`STATE_ENV`] [`STATE`]. Always on while
/// [`SWITCH`] is (the owner on #6226: every lamp has 6 GB): enabled at
/// boot ([`WANTS`], what `systemctl enable` makes) once OpenClaw is there,
/// started by the round and restarted by an install; disabled and stopped
/// with the switch off. Its memory (#6226: RAM floor): no `MemoryHigh`
/// (at 700M on lamp-c0a4 it throttled the gateway into 18 s stalls and no
/// turn finished), only `MemoryMax=2500M` as a guard, and Node's own heap
/// limit left as it is. Its CPU caps are sane values, not measured yet:
/// below the lamp's services (`Nice`, `CPUWeight`, `CPUQuota`), started
/// again on failure, given up on after five starts in ten minutes.
pub const UNIT_TEXT: &str = "\
# lamp-openclaw-gateway: OpenClaw's gateway on the lamp, which Harness's OpenClaw panes talk to (lampOS/docs/install.md,
# \"OpenClaw on the lamp\", #6226). Written by a round of lamp-update with LAMP_OPENCLAW_ON_LAMP on, enabled and always
# on while it is (the owner: every lamp has 6 GB), disabled and stopped with it off. It runs only through
# /usr/local/bin/openclaw, in the namespace that wrapper makes, as lampos, on loopback port 18789 with its own token.
[Unit]
Description=lampOS OpenClaw gateway for Harness (lamp-openclaw-gateway)
After=network-online.target lamp-cloud.service
StartLimitIntervalSec=600
StartLimitBurst=5

[Service]
Type=simple
User=lampos
Group=lampos
Environment=HOME=/var/lib/lampos-openclaw OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state
WorkingDirectory=/var/lib/lampos-openclaw/state
ExecStart=/usr/local/bin/openclaw gateway run
Restart=on-failure
RestartSec=10
StandardInput=null
SyslogIdentifier=lamp-openclaw-gateway
Nice=10
CPUWeight=20
CPUQuota=100%
# (#6226: RAM floor) on lamp-c0a4 a memory throttle stalled the gateway (about 780k throttles, 18 s stalls, no turn
# finished), so none is set: only a hard guard, and Node's own heap limit left as it is.
MemoryMax=2500M
TasksMax=256
NoNewPrivileges=yes

[Install]
WantedBy=multi-user.target
";

/// What `systemctl enable` makes of [`UNIT_TEXT`]'s `WantedBy`: a link in
/// `multi-user.target.wants` to the unit. The round makes and removes it.
pub const WANTS: &str = "/etc/systemd/system/multi-user.target.wants/lamp-openclaw-gateway.service";

/// Set: `on` exactly; unset: `default` on a product lamp, off on another.
pub fn wanted(switch: Option<&str>, product: bool, default: bool) -> bool {
    hermes_install::wanted(switch, product, default)
}

/// Where OpenClaw's install is, as [`RECORD`] keeps it: `install` one of
/// [`OpenclawInstall::INSTALL`], `error` (with `failed` only) one of
/// [`OpenclawInstall::ERRORS`]; `version` and `node` with `installed` only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenclawInstall {
    pub install: String,
    pub revision: u64,
    pub at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl OpenclawInstall {
    /// As Hermes's.
    pub const INSTALL: [&str; 4] = hermes_install::HermesInstall::INSTALL;
    /// Hermes's, with `fetch` (Node or the package not fetched) and
    /// `gateway` (its unit not there, or a running one not started again); a pin
    /// that differs is `pin_mismatch`, a Node or package not at the pin
    /// after its step `version`, a Node below what OpenClaw runs on
    /// ([`node_supported`]) `old_node`.
    pub const ERRORS: [&str; 18] = [
        "pin_mismatch",
        "no_user",
        "no_space",
        "no_memory",
        "no_tools",
        "no_userns",
        "serve",
        "timeout",
        "fetch",
        "installer",
        "version",
        "old_node",
        "config",
        "entry",
        "gateway",
        "not_started",
        "privacy_lost",
        "interrupted",
    ];

    fn new(install: &str, error: Option<&str>, at_unix_ms: u64) -> Self {
        Self {
            install: install.to_owned(),
            revision: 0,
            at_unix_ms,
            version: None,
            node: None,
            error: error.map(str::to_owned),
        }
    }

    fn installed(at_unix_ms: u64) -> Self {
        Self {
            version: Some(VERSION.to_owned()),
            node: Some(NODE_VERSION.to_owned()),
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
            && plain(&self.node)
    }
}

/// What the round hands the one-shot in [`ASK`], as Hermes's.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Ask {
    version: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    remove: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    skills: bool,
}

/// The install as [`RECORD`] has it, when lamp-update could have written
/// it; else none.
pub fn record(records: &Path) -> Option<OpenclawInstall> {
    let bytes = fs::read(records.join(RECORD)).ok()?;
    serde_json::from_slice::<OpenclawInstall>(&bytes)
        .ok()
        .filter(OpenclawInstall::check)
}

fn write(records: &Path, mut said: OpenclawInstall) -> OpenclawInstall {
    said.revision = record(records).map_or(1, |before| before.revision.saturating_add(1));
    if let Ok(text) = serde_json::to_vec(&said) {
        let _ = write_whole(&records.join(RECORD), &text, 0o600);
    }
    said
}

fn failed(at_unix_ms: u64, error: &str) -> OpenclawInstall {
    OpenclawInstall::new("failed", Some(error), at_unix_ms)
}

/// Whether an OpenClaw install is under way as the record says.
pub fn installing(records: &Path) -> bool {
    record(records).is_some_and(|said| said.install == "installing")
}

/// Whether an OpenClaw install is under way and not taken as stuck: what
/// holds Hermes's ask to a later round.
pub fn under_way(records: &Path, now_unix_ms: u64) -> bool {
    record(records).is_some_and(|said| {
        said.install == "installing"
            && now_unix_ms.saturating_sub(said.at_unix_ms) < STUCK_AFTER.as_millis() as u64
    })
}

/// Whether Hermes's install is under way and not taken as stuck: what
/// holds OpenClaw's ask to a later round.
fn hermes_under_way(records: &Path, now_unix_ms: u64) -> bool {
    hermes_install::record(records).is_some_and(|said| {
        said.install == "installing"
            && now_unix_ms.saturating_sub(said.at_unix_ms)
                < hermes_install::STUCK_AFTER.as_millis() as u64
    })
}

/// OpenClaw's turn, for the watch ([`crate::hermes_schedule::carry_out_turns`]).
pub const TURN: hermes_install::Turn = hermes_install::Turn {
    name: "OpenClaw",
    installing,
    stopped: |records, error, now_unix_ms| {
        stopped(records, error, now_unix_ms);
    },
};

/// An install the watch stopped: recorded `failed` with `error`, its
/// [`ASK`] taken away.
pub fn stopped(records: &Path, error: &str, now_unix_ms: u64) -> OpenclawInstall {
    let _ = fs::remove_file(records.join(ASK));
    write(records, failed(now_unix_ms, error))
}

/// The installed package's version, from its `package.json`.
fn package_version(root: &Path) -> Option<String> {
    let bytes = fs::read(on(root, PACKAGE_JSON)).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    json.get("version")?.as_str().map(str::to_owned)
}

/// The unpacked Node's version, from its `node_version.h` (never run).
fn node_version(root: &Path) -> Option<String> {
    let text = fs::read_to_string(on(root, NODE_VERSION_H)).ok()?;
    let part = |name: &str| {
        text.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("#define") && words.next() == Some(name))
                .then(|| words.next())
                .flatten()
                .filter(|value| value.bytes().all(|b| b.is_ascii_digit()))
                .map(str::to_owned)
        })
    };
    Some(format!(
        "{}.{}.{}",
        part("NODE_MAJOR_VERSION")?,
        part("NODE_MINOR_VERSION")?,
        part("NODE_PATCH_VERSION")?
    ))
}

/// Whether OpenClaw runs on Node `version`: its package's `engines`,
/// `>=24.16.0 <25 || >=26.1.0`.
pub fn node_supported(version: &str) -> bool {
    let parts: Vec<u32> = version
        .split('.')
        .map_while(|part| part.parse().ok())
        .collect();
    match parts[..] {
        [major, minor, _] => {
            (major == 24 && minor >= 16) || (major == 26 && minor >= 1) || major > 26
        }
        _ => false,
    }
}

/// The unit's path under `root`.
fn unit_path(root: &Path) -> PathBuf {
    root.join(crate::writer_unit::FOLDER).join(UNIT)
}

/// OpenClaw there as this build puts it, under `root`: the package at
/// [`VERSION`] with its command, installed from this build's
/// [`LOCKFILE`], the Node at [`NODE_VERSION`], the wrapper
/// and namespace script with this build's text (the script's as
/// [`ns_text`]`(skills)`), and the gateway's unit.
pub fn present(root: &Path, skills: bool) -> bool {
    fs::metadata(on(root, OPENCLAW)).is_ok()
        && package_version(root).as_deref() == Some(VERSION)
        && holds(root, LOCKFILE_AT, LOCKFILE)
        && node_version(root).as_deref() == Some(NODE_VERSION)
        && holds(root, WRAPPER, WRAPPER_TEXT)
        && holds(root, NS, &ns_text(skills))
        && fs::read(unit_path(root)).is_ok_and(|bytes| bytes == UNIT_TEXT.as_bytes())
}

/// When `serve` starts, as Hermes's: off, an earlier record and ask are
/// taken away; on with OpenClaw missing and no record, `absent`.
pub fn at_start(root: &Path, records: &Path, switch: bool, skills: bool, now_unix_ms: u64) {
    if !switch {
        for name in [RECORD, ASK] {
            let _ = fs::remove_file(records.join(name));
        }
        return;
    }
    if !present(root, skills) && record(records).is_none() {
        write(records, OpenclawInstall::new("absent", None, now_unix_ms));
    }
}

fn ask(records: &Path, remove: bool, skills: bool) -> io::Result<()> {
    let text = serde_json::to_vec(&Ask {
        version: 1,
        remove,
        skills,
    })
    .map_err(io::Error::other)?;
    write_whole(&records.join(ASK), &text, 0o600)
}

/// An install [`ASK`] taken back (an ask to remove is kept).
pub fn withhold(records: &Path) -> bool {
    let Ok(bytes) = fs::read(records.join(ASK)) else {
        return false;
    };
    let asked = serde_json::from_slice::<Ask>(&bytes).unwrap_or_default();
    !asked.remove && fs::remove_file(records.join(ASK)).is_ok()
}

/// After a round that reached the channel, with the switch on, after
/// Hermes's own [`hermes_install::after_round`]: as Hermes's, but first
/// the gateway's unit is written (a round may, the one-shot may not) and
/// systemd reloaded when it changed, and Hermes's install under way holds
/// this one to a later round, its turn in the one-shot.
pub fn after_round(
    root: &Path,
    records: &Path,
    skills: bool,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    window: Result<crate::hermes_schedule::Window, String>,
    say: &mut dyn FnMut(&str),
) -> Option<OpenclawInstall> {
    match crate::writer_unit::write_unit(root, UNIT, UNIT_TEXT) {
        Ok(true) => {
            if let Err(why) = systemd.reload() {
                say(&format!("{UNIT} written, but systemd not reloaded: {why}"));
            }
        }
        Ok(false) => {}
        Err(why) => say(&format!("OpenClaw's gateway unit not written: {why}")),
    }
    let before = record(records);
    if present(root, skills) {
        gateway_on(root, systemd, say);
        if before
            .as_ref()
            .is_some_and(|said| said.install == "installed")
        {
            return None;
        }
        say(&format!(
            "OpenClaw {VERSION} is at {OPENCLAW}: recorded as installed"
        ));
        return Some(write(records, OpenclawInstall::installed(now_unix_ms)));
    }
    let since = |said: &OpenclawInstall| now_unix_ms.saturating_sub(said.at_unix_ms);
    if let Some(said) = &before {
        if said.install == "installing" && since(said) < STUCK_AFTER.as_millis() as u64 {
            return None;
        }
        if said.install == "failed"
            && !matches!(said.error.as_deref(), Some(error) if RETRY_IN_NEXT_WINDOW.contains(&error))
            && since(said) < RETRY_AFTER.as_millis() as u64
        {
            return None;
        }
    }
    let window = match window {
        Ok(window) => window,
        Err(why) => {
            if withhold(records) {
                say("OpenClaw's install asked earlier is taken back: not its window");
            }
            say(&format!(
                "OpenClaw {VERSION} is not set up yet: it waits, {why}"
            ));
            return None;
        }
    };
    if hermes_under_way(records, now_unix_ms) {
        say("OpenClaw waits its turn: Hermes's install is under way in Harness's one-shot");
        return None;
    }
    say(&format!("OpenClaw may be installed now: {}", window.name()));
    if let Err(error) = ask(records, false, skills) {
        say(&format!(
            "OpenClaw is not installed: {ASK} not written: {error}"
        ));
        return Some(write(records, failed(now_unix_ms, "not_started")));
    }
    if !harness_install::present(root) {
        say("OpenClaw waits for Harness: Harness's one-shot installs it after Harness and Hermes");
        return match before {
            Some(said) if said.install == "absent" => None,
            _ => Some(write(
                records,
                OpenclawInstall::new("absent", None, now_unix_ms),
            )),
        };
    }
    write(
        records,
        OpenclawInstall::new("installing", None, now_unix_ms),
    );
    match harness_install::start(root, systemd) {
        Ok(()) => {
            say(&format!(
                "OpenClaw {VERSION} is not set up at {OPENCLAW}: {} started to put it there",
                harness_install::UNIT
            ));
            record(records)
        }
        Err(why) => {
            say(&format!(
                "OpenClaw is not installed: {} not started: {why}",
                harness_install::UNIT
            ));
            Some(write(records, failed(now_unix_ms, "not_started")))
        }
    }
}

/// The gateway always on with OpenClaw there (the owner on #6226): enabled
/// at boot by [`WANTS`] (made when missing, as `systemctl enable` does,
/// systemd then reloaded) and started (a no-op when it runs).
fn gateway_on(root: &Path, systemd: &mut dyn Systemd, say: &mut dyn FnMut(&str)) {
    let wants = on(root, WANTS);
    let target = format!("/{}/{UNIT}", crate::writer_unit::FOLDER);
    if fs::read_link(&wants).ok().as_deref() != Some(Path::new(&target)) {
        let made = wants
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| match fs::symlink_metadata(&wants) {
                Ok(_) => fs::remove_file(&wants),
                Err(_) => Ok(()),
            })
            .and_then(|()| std::os::unix::fs::symlink(&target, &wants));
        match made {
            Ok(()) => {
                say(&format!("{UNIT} enabled: OpenClaw's gateway is always on"));
                if let Err(why) = systemd.reload() {
                    say(&format!("systemd not reloaded: {why}"));
                }
            }
            Err(error) => say(&format!("{UNIT} not enabled: {error}")),
        }
    }
    if let Err(why) = systemd.start_unit(UNIT) {
        say(&format!("{UNIT} not started: {why}"));
    }
}

/// The gateway disabled ([`WANTS`] taken out, systemd reloaded) and
/// stopped, with the switch off.
fn gateway_off(root: &Path, systemd: &mut dyn Systemd, say: &mut dyn FnMut(&str)) {
    let wants = on(root, WANTS);
    if fs::symlink_metadata(&wants).is_ok() {
        match fs::remove_file(&wants) {
            Ok(()) => {
                say(&format!("{UNIT} disabled: OpenClaw on the lamp is off"));
                if let Err(why) = systemd.reload() {
                    say(&format!("systemd not reloaded: {why}"));
                }
            }
            Err(error) => say(&format!("{UNIT} not disabled: {error}")),
        }
    }
    if fs::symlink_metadata(unit_path(root)).is_ok()
        && let Err(why) = systemd.stop(UNIT)
    {
        say(&format!("{UNIT} not stopped: {why}"));
    }
}

/// After a round that reached the channel, with the switch off: the
/// gateway disabled and stopped ([`gateway_off`]), then an entry
/// point still there is taken out by Harness's one-shot (which stops the
/// gateway first), asked and started without waiting. With
/// none left, the round takes the gateway's unit out itself and reloads
/// systemd. Whether the one-shot was started is returned.
pub fn after_round_off(
    root: &Path,
    records: &Path,
    systemd: &mut dyn Systemd,
    say: &mut dyn FnMut(&str),
) -> bool {
    gateway_off(root, systemd, say);
    let there = [WRAPPER, NS]
        .into_iter()
        .any(|path| fs::symlink_metadata(on(root, path)).is_ok());
    if !there {
        if fs::symlink_metadata(unit_path(root)).is_ok() {
            match fs::remove_file(unit_path(root)) {
                Ok(()) => {
                    say(&format!("OpenClaw on the lamp is off: {UNIT} taken out"));
                    if let Err(why) = systemd.reload() {
                        say(&format!("systemd not reloaded: {why}"));
                    }
                }
                Err(error) => say(&format!("{UNIT} not taken out: {error}")),
            }
        }
        return false;
    }
    let started = ask(records, true, false)
        .map_err(|error| error.to_string())
        .and_then(|()| harness_install::start(root, systemd));
    match started {
        Ok(()) => {
            say(&format!(
                "OpenClaw on the lamp is off: {} started to stop its gateway and take its wrapper out",
                harness_install::UNIT
            ));
            true
        }
        Err(why) => {
            say(&format!(
                "OpenClaw on the lamp is off, but its wrapper stays: {why}"
            ));
            false
        }
    }
}

/// What an install takes: this build's pins; a test's may be other bytes'.
struct Kit {
    node_url: &'static str,
    node_sha256: &'static str,
    node_version: &'static str,
    lockfile: &'static str,
    lockfile_sha256: &'static str,
    version: &'static str,
}

impl Kit {
    const BUILT: Kit = Kit {
        node_url: NODE_URL,
        node_sha256: NODE_SHA256,
        node_version: NODE_VERSION,
        lockfile: LOCKFILE,
        lockfile_sha256: LOCKFILE_SHA256,
        version: VERSION,
    };

    fn built(&self) -> bool {
        self.node_sha256 == NODE_SHA256 && self.lockfile_sha256 == LOCKFILE_SHA256
    }
}

/// Whether `lockfile` is the pinned one: its SHA-256 `sha256`, a lockfile
/// npm 7 or later reads (version 2 or 3), whose project wants `openclaw`
/// at `version` exactly (as [`MANIFEST`]), and whose `openclaw` is
/// `version` from [`PACKAGE_URL`] with [`PACKAGE_INTEGRITY`]. Every other
/// package of the tree must carry a registry tarball and its integrity,
/// unless it comes inside another package's tarball (`inBundle`).
pub fn lockfile_pinned(lockfile: &str, sha256: &str, version: &str) -> Result<(), String> {
    let found = crate::manifest::sha256_hex(lockfile.as_bytes());
    if found != sha256 {
        return Err(format!("the lockfile's sha256 is {found}, not {sha256}"));
    }
    let lock: serde_json::Value =
        serde_json::from_str(lockfile).map_err(|error| format!("the lockfile: {error}"))?;
    if !matches!(lock["lockfileVersion"].as_u64(), Some(2 | 3)) {
        return Err("the lockfile is not npm's version 2 or 3".to_owned());
    }
    let packages = lock["packages"]
        .as_object()
        .ok_or("the lockfile lists no packages")?;
    let wants = &packages.get("").ok_or("the lockfile has no project")?["dependencies"];
    if wants != &serde_json::json!({"openclaw": version}) {
        return Err(format!(
            "the lockfile's project wants {wants}, not openclaw {version}"
        ));
    }
    let openclaw = packages
        .get("node_modules/openclaw")
        .ok_or("the lockfile has no openclaw")?;
    let pinned = serde_json::json!([version, PACKAGE_URL, PACKAGE_INTEGRITY]);
    let given = serde_json::json!([
        openclaw["version"],
        openclaw["resolved"],
        openclaw["integrity"]
    ]);
    if given != pinned {
        return Err(format!("the lockfile's openclaw is {given}, not {pinned}"));
    }
    for (name, package) in packages {
        let bundled = package["inBundle"].as_bool() == Some(true);
        let fetched = package["resolved"]
            .as_str()
            .is_some_and(|url| url.starts_with("https://registry.npmjs.org/"))
            && package["integrity"]
                .as_str()
                .is_some_and(|integrity| integrity.starts_with("sha512-"));
        if !name.is_empty() && !bundled && !fetched {
            return Err(format!(
                "the lockfile's {name} has no registry tarball with a sha512 integrity"
            ));
        }
    }
    Ok(())
}

/// `lamp-update harness-install`'s OpenClaw part, after Hermes's: none
/// when no [`ASK`] is there. Asked to remove, the gateway is stopped and
/// the entry points taken out, the record with them. Else
/// [`steps`]; a failure takes the entry points out. An error only when
/// another install holds [`LOCK`].
pub fn install_now(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
) -> Result<Option<OpenclawInstall>, String> {
    install_with(root, records, run, now_unix_ms, say, &Kit::BUILT)
}

fn install_with(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
    kit: &Kit,
) -> Result<Option<OpenclawInstall>, String> {
    let Ok(bytes) = fs::read(records.join(ASK)) else {
        return Ok(None);
    };
    let asked = serde_json::from_slice::<Ask>(&bytes).unwrap_or_default();
    let _lock = lock_named(records, LOCK, "OpenClaw")?;
    let said = if asked.remove {
        remove_entry_points(root, run, say);
        let _ = fs::remove_file(records.join(RECORD));
        None
    } else {
        let said = steps(root, records, run, now_unix_ms, say, kit, asked.skills);
        if said.install == "failed" {
            remove_entry_points(root, run, say);
        }
        Some(said)
    };
    let _ = fs::remove_file(records.join(ASK));
    Ok(said)
}

/// The gateway stopped, then the wrapper and the namespace
/// script taken out, the wrapper first. OpenClaw's folder (its state among
/// it) and the unit file (the round's) stay.
fn remove_entry_points(root: &Path, run: &mut Runner, say: &mut dyn FnMut(&str)) {
    if fs::symlink_metadata(unit_path(root)).is_ok()
        && let Err(why) = run(&["systemctl", "stop", UNIT].map(str::to_owned))
    {
        say(&format!("{UNIT} not stopped: {why}"));
    }
    for path in [WRAPPER, NS] {
        match fs::remove_file(on(root, path)) {
            Ok(()) => say(&format!("{path} taken out")),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => say(&format!("{path} not taken out: {error}")),
        }
    }
}

/// The install, in order: `lampos` and its ids looked up; `curl`, `tar`
/// and `gzip` looked for as `lampos`, `unshare` at its path; the
/// namespace tried as `lampos`; the lockfile checked ([`lockfile_pinned`]).
/// Then, unless the pinned Node and package are in [`PLACE`] already
/// (`lampos`'s): memory and free space checked, another OpenClaw's Node
/// and package moved aside to [`ASIDE`], [`PLACE`] and [`STATE`] made
/// `lampos`'s 0700, the Node tarball fetched, checked and unpacked as
/// `lampos`, the namespace script written, [`MANIFEST`] and the lockfile
/// written in [`PREFIX`] (root's, 0644), and `npm ci` run there by the
/// Node's npm as `lampos` inside the namespace ([`NPM_CI_ENV`]). Then the
/// gateway's token made once, OpenClaw's config given its `gateway` block,
/// `openclaw --version` run in the namespace (OpenClaw's first run, which
/// finishes its package step), the namespace script and the wrapper
/// written, the gateway (re)started (the round enables it), [`ASIDE`]
/// removed, and after a new install lamp-tasks started again if it runs.
fn steps(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
    kit: &Kit,
    skills: bool,
) -> OpenclawInstall {
    if present(root, skills) && kit.built() {
        say(&format!(
            "OpenClaw {VERSION} is already at {OPENCLAW}: nothing installed"
        ));
        if let Some(said) = record(records)
            && said.install == "installed"
        {
            return said;
        }
        return write(records, OpenclawInstall::installed(now_unix_ms()));
    }
    write(
        records,
        OpenclawInstall::new("installing", None, now_unix_ms()),
    );
    let mut fail = |error: &str, why: String, say: &mut dyn FnMut(&str)| {
        say(&format!("OpenClaw is not installed: {why}"));
        write(records, failed(now_unix_ms(), error))
    };
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
             OpenClaw would see the lamp's services' files, so nothing is installed"
        );
        return fail("no_userns", why, say);
    }
    if let Err(why) = lockfile_pinned(kit.lockfile, kit.lockfile_sha256, kit.version) {
        return fail("pin_mismatch", why, say);
    }
    let pinned_there = fs::metadata(on(root, OPENCLAW)).is_ok()
        && package_version(root).as_deref() == Some(kit.version)
        && holds(root, LOCKFILE_AT, kit.lockfile)
        && node_version(root).as_deref() == Some(kit.node_version)
        && owner(root, PLACE).as_deref() == Some(uid.as_str());
    if !pinned_there {
        match available_kb(root) {
            Some(available) if available >= NEEDS_AVAILABLE_KB => {}
            available => {
                let why = match available {
                    Some(available) => format!(
                        "{available} KiB of memory available, under the {NEEDS_AVAILABLE_KB} KiB it needs"
                    ),
                    None => format!("MemAvailable not read from {}", hermes_install::MEMINFO),
                };
                return fail("no_memory", why, say);
            }
        }
        match free_kb(run) {
            Some(free) if free >= NEEDS_FREE_KB => {}
            free => {
                let why = match free {
                    Some(free) => format!(
                        "{free} KiB free in /var/lib, under the {NEEDS_FREE_KB} KiB it needs"
                    ),
                    None => "the free space in /var/lib not read".to_owned(),
                };
                return fail("no_space", why, say);
            }
        }
        if let Err(why) = set_aside(root, &uid, say) {
            return fail("serve", why, say);
        }
        for (folder, mode) in [
            (PLACE, "0700"),
            (STATE, "0700"),
            (NODE_DIR, "0755"),
            (PREFIX, "0755"),
        ] {
            if let Err(why) = run(
                &["install", "-d", "-o", USER, "-g", USER, "-m", mode, folder].map(str::to_owned),
            ) {
                return fail("serve", format!("{folder} not made {USER}'s: {why}"), say);
            }
        }
        let downloads = match Downloads::make() {
            Ok(downloads) => downloads,
            Err(error) => {
                return fail(
                    "serve",
                    format!("no folder for the fetched files: {error}"),
                    say,
                );
            }
        };
        let node_file = downloads.file("node.tar.gz");
        if let Err((error, why)) = fetch_checked(run, kit.node_url, &node_file, &|bytes| {
            let found = crate::manifest::sha256_hex(bytes);
            (found == kit.node_sha256)
                .then_some(())
                .ok_or(format!("sha256 {found}, not {}", kit.node_sha256))
        }) {
            return fail(error, format!("Node {}: {why}", kit.node_version), say);
        }
        let node_file = node_file.to_string_lossy().into_owned();
        let unpack = [
            "tar",
            "-xzf",
            node_file.as_str(),
            "-C",
            NODE_DIR,
            "--strip-components=1",
            "--no-same-owner",
        ];
        if let Err(why) = run(&as_lampos(&unpack)) {
            return fail(long_error(&why), format!("Node not unpacked: {why}"), say);
        }
        let have = node_version(root);
        if let Some(found) = have.as_deref()
            && !node_supported(found)
        {
            let why = format!(
                "the Node unpacked is {found}, below what OpenClaw runs on (>=24.16.0 <25 or >=26.1.0)"
            );
            return fail("old_node", why, say);
        }
        if have.as_deref() != Some(kit.node_version) {
            let why = format!(
                "the Node unpacked is {}, not the pinned {}",
                have.as_deref().unwrap_or("no version"),
                kit.node_version
            );
            return fail("version", why, say);
        }
        drop(downloads);
        // The namespace script first: npm runs inside it.
        let ns = ns_text(skills);
        if let Err(why) = write_root_file(root, NS, &ns, 0o755) {
            return fail("entry", why, say);
        }
        for (path, text) in [(MANIFEST_AT, MANIFEST), (LOCKFILE_AT, kit.lockfile)] {
            if let Err(why) = write_root_file(root, path, text, 0o644) {
                return fail("serve", why, say);
            }
        }
        let started = std::time::Instant::now();
        if let Err(why) = run(&in_namespace(&uid, &gid, true, &[])) {
            return fail(long_error(&why), format!("npm ci: {why}"), say);
        }
        say(&format!(
            "OpenClaw's package installed by npm ci from the pinned lockfile in {} s",
            started.elapsed().as_secs()
        ));
        if !holds(root, LOCKFILE_AT, kit.lockfile) {
            return fail("installer", format!("{LOCKFILE_AT} changed by npm ci"), say);
        }
        if fs::metadata(on(root, OPENCLAW)).is_err() {
            return fail(
                "installer",
                format!("no openclaw at {OPENCLAW} after npm"),
                say,
            );
        }
        let have = package_version(root);
        if have.as_deref() != Some(kit.version) {
            let why = format!(
                "the package installed is {}, not the pinned {}",
                have.as_deref().unwrap_or("no version"),
                kit.version
            );
            return fail("version", why, say);
        }
    }
    if let Err(why) = gateway_config(root, run, say) {
        return fail("config", why, say);
    }
    match run(&in_namespace(&uid, &gid, false, &["--version"])) {
        Ok(said) => say(&format!(
            "OpenClaw runs: {}",
            said.lines().next().unwrap_or("").trim()
        )),
        Err(why) => {
            return fail(
                "installer",
                format!("the new OpenClaw does not run: {why}"),
                say,
            );
        }
    }
    if let Err(why) = write_entry_points(root, skills) {
        return fail("entry", why, say);
    }
    if !fs::read(unit_path(root)).is_ok_and(|bytes| bytes == UNIT_TEXT.as_bytes()) {
        return fail(
            "gateway",
            format!("{UNIT} is not this build's: a round writes it"),
            say,
        );
    }
    // Always on (the owner on #6226): started now, or restarted so one
    // running an older OpenClaw runs the new one. The round enables it.
    if let Err(why) = run(&["systemctl", "restart", UNIT].map(str::to_owned)) {
        return fail("gateway", format!("systemctl restart {UNIT}: {why}"), say);
    }
    remove_aside(root, say);
    if !pinned_there
        && let Err(why) =
            run(&["systemctl", "try-restart", "lamp-tasks.service"].map(str::to_owned))
    {
        // lamp-tasks merges OpenClaw's models and skills at its start.
        say(&format!("lamp-tasks.service not started again: {why}"));
    }
    let shown = if skills {
        "its own folder, its work folder, the skills and lamp-skills' socket"
    } else {
        "its own folder, its work folder and the skills"
    };
    say(&format!(
        "OpenClaw {} on Node {} installed for {USER} at {OPENCLAW}; Harness runs it through {WRAPPER}, its gateway {UNIT} (127.0.0.1:{PORT}) always on, in a namespace that shows it only {shown}",
        kit.version, kit.node_version
    ));
    write(records, OpenclawInstall::installed(now_unix_ms()))
}

/// The error for a long step that failed: `timeout` past its bound, else
/// `installer`.
fn long_error(why: &str) -> &'static str {
    if install::timed_out(why) {
        "timeout"
    } else {
        "installer"
    }
}

/// `url` fetched by `curl` (https only) into `file`, then its bytes
/// checked by `check`: not fetched is `fetch` (or `timeout`), other bytes
/// `pin_mismatch`.
fn fetch_checked(
    run: &mut Runner,
    url: &str,
    file: &Path,
    check: &dyn Fn(&[u8]) -> Result<(), String>,
) -> Result<(), (&'static str, String)> {
    let out = file.to_string_lossy().into_owned();
    let argv = [
        "curl",
        "-fsSL",
        "--proto",
        "=https",
        "--tlsv1.2",
        "--max-filesize",
        MOST_BYTES,
        "-o",
        out.as_str(),
        url,
    ]
    .map(str::to_owned);
    if let Err(why) = run(&argv) {
        let error = if install::timed_out(&why) {
            "timeout"
        } else {
            "fetch"
        };
        return Err((error, format!("not fetched from {url}: {why}")));
    }
    let bytes =
        fs::read(file).map_err(|error| ("fetch", format!("{}: {error}", file.display())))?;
    check(&bytes).map_err(|why| ("pin_mismatch", format!("{url} gave {why}")))?;
    // Read by lampos (tar, npm), changed by none but root.
    let _ = fs::set_permissions(file, fs::Permissions::from_mode(0o644));
    Ok(())
}

/// npm's integrity of `bytes`: `sha512-` and the digest in base64.
pub fn integrity_of(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA512, bytes);
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(digest.as_ref())
    )
}

/// Whether `argv` is one of OpenClaw's long steps: one that names a file in
/// its downloads folder (a fetch, the Node's unpacking), or `npm ci`
/// ([`NPM_CI_ENV`]). [`install::output_of`] gives each [`STEP_WITHIN`].
pub fn runs_long_step(argv: &[String]) -> bool {
    let npm_ci = format!("{NPM_CI_ENV}=on");
    argv.iter()
        .any(|arg| arg.contains(&format!("/{DOWNLOADS}")) || *arg == npm_ci)
}

/// The gateway's token made once (32 random bytes, hex; `lampos`'s, 0600)
/// and OpenClaw's config given its `gateway` block: written when there is
/// none, the block set in one there (every other key kept) when it lacks
/// it or the token was just made. A config lamp-update cannot read as a
/// JSON object is left as it is, and said.
fn gateway_config(root: &Path, run: &mut Runner, say: &mut dyn FnMut(&str)) -> Result<(), String> {
    let token_path = on(root, GATEWAY_TOKEN);
    let kept = fs::symlink_metadata(&token_path)
        .ok()
        .filter(|meta| meta.is_file())
        .and_then(|_| fs::read_to_string(&token_path).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    let (token, made) = match kept {
        Some(token) => (token, false),
        None => {
            let mut bytes = [0u8; 32];
            ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
                .map_err(|_| "no random bytes for the gateway's token".to_owned())?;
            let token = crate::manifest::hex(&bytes);
            write_whole(&token_path, format!("{token}\n").as_bytes(), 0o600)
                .map_err(|error| format!("{GATEWAY_TOKEN}: {error}"))?;
            give_to_lampos(run, GATEWAY_TOKEN)?;
            (token, true)
        }
    };
    let gateway = serde_json::json!({
        "mode": "local",
        "bind": "loopback",
        "port": PORT,
        "auth": {"mode": "token", "token": token},
    });
    let config_path = on(root, CONFIG);
    let mut config = match fs::read(&config_path) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(serde_json::Value::Object(config)) => config,
            _ => {
                say(&format!(
                    "{CONFIG} is not a JSON object lamp-update reads: left as it is"
                ));
                return Ok(());
            }
        },
        Err(error) if error.kind() == ErrorKind::NotFound => serde_json::Map::new(),
        Err(error) => return Err(format!("{CONFIG}: {error}")),
    };
    if !made && config.contains_key("gateway") {
        return Ok(());
    }
    config.insert("gateway".to_owned(), gateway);
    let text = serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?;
    write_whole(&config_path, &text, 0o600).map_err(|error| format!("{CONFIG}: {error}"))?;
    give_to_lampos(run, CONFIG)
}

/// `path` made `lampos`'s (a link as a link).
fn give_to_lampos(run: &mut Runner, path: &str) -> Result<(), String> {
    run(&["chown", "-h", &format!("{USER}:{USER}"), path].map(str::to_owned))
        .map(drop)
        .map_err(|why| format!("{path} not made {USER}'s: {why}"))
}

/// What the install needs on the lamp: `curl` (run as root), `tar` and
/// `gzip` (as `lampos`, to unpack Node), looked for with the install's
/// `PATH`, and `unshare` for the wrapper.
fn tool_checks() -> Vec<(&'static str, Vec<String>)> {
    ["curl", "tar", "gzip"]
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

/// `args` as `lampos`, with [`PLACE`] as `HOME` (npm's cache goes there),
/// OpenClaw's state folder, [`INSTALL_PATH`] and `SHELL=/bin/sh`.
fn as_lampos(args: &[&str]) -> Vec<String> {
    let mut argv: Vec<String> = ["runuser", "-u", USER, "--", "env"]
        .map(str::to_owned)
        .into();
    argv.extend([
        format!("HOME={PLACE}"),
        format!("{STATE_ENV}={STATE}"),
        format!("PATH={INSTALL_PATH}"),
        "SHELL=/bin/sh".to_owned(),
    ]);
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    argv
}

/// `args` as `lampos` inside OpenClaw's namespace, as [`WRAPPER`] runs it
/// ([`NS_FLAGS`], then [`NS`] with the caller's ids), but with no token:
/// with `npm_ci`, [`NS`]'s install mode ([`NPM_CI_ENV`]), else OpenClaw.
fn in_namespace(uid: &str, gid: &str, npm_ci: bool, args: &[&str]) -> Vec<String> {
    let ids = [format!("LAMP_NS_UID={uid}"), format!("LAMP_NS_GID={gid}")];
    let mut tail: Vec<&str> = ids.iter().map(String::as_str).collect();
    let npm_ci_on = format!("{NPM_CI_ENV}=on");
    if npm_ci {
        tail.push(&npm_ci_on);
    }
    tail.push("/usr/bin/unshare");
    tail.extend(NS_FLAGS);
    tail.push(NS);
    tail.extend(args);
    as_lampos(&tail)
}

/// `text` written whole at `path` under `root` (beside, then renamed in),
/// root's, with `mode`; its folder must be there.
fn write_root_file(root: &Path, path: &str, text: &str, mode: u32) -> Result<(), String> {
    let place = on(root, path);
    if let Some(folder) = place.parent()
        && !folder.is_dir()
    {
        return Err(format!("{}: no such folder", folder.display()));
    }
    write_whole(&place, text.as_bytes(), mode).map_err(|error| format!("{path}: {error}"))
}

/// The namespace script, then the wrapper: root's, 0755, each written
/// whole beside and renamed in.
fn write_entry_points(root: &Path, skills: bool) -> Result<(), String> {
    let ns = ns_text(skills);
    for (path, text) in [(NS, ns.as_str()), (WRAPPER, WRAPPER_TEXT)] {
        write_root_file(root, path, text, 0o755)?;
    }
    Ok(())
}

/// An OpenClaw in [`PLACE`] that is not this build's moved out of the
/// way. The folder another user's (or not a folder): moved whole to
/// [`ASIDE`] (a link removed, never followed). Else its Node and package,
/// when there, moved into [`ASIDE`], its state kept. One there already
/// from an earlier try is kept, and these removed instead.
fn set_aside(root: &Path, uid: &str, say: &mut dyn FnMut(&str)) -> Result<(), String> {
    let place = on(root, PLACE);
    let Ok(meta) = fs::symlink_metadata(&place) else {
        return Ok(());
    };
    if !meta.is_dir() {
        return fs::remove_file(&place).map_err(|error| format!("{PLACE}: {error}"));
    }
    let aside = on(root, ASIDE);
    let aside_there = fs::symlink_metadata(&aside).is_ok();
    if meta.uid().to_string() != uid {
        if aside_there {
            say(&format!(
                "{PLACE} is another user's, and one is already aside at {ASIDE}: this one removed"
            ));
            return fs::remove_dir_all(&place)
                .map_err(|error| format!("{PLACE} not removed: {error}"));
        }
        say(&format!(
            "{PLACE} is another user's: moved aside to {ASIDE}"
        ));
        return fs::rename(&place, &aside)
            .map_err(|error| format!("{PLACE} not moved aside: {error}"));
    }
    for (path, name) in [(NODE_DIR, "node"), (PREFIX, "npm")] {
        let from = on(root, path);
        if fs::symlink_metadata(&from).is_err() {
            continue;
        }
        if aside_there {
            let removed = if from.is_dir() && !from.is_symlink() {
                fs::remove_dir_all(&from)
            } else {
                fs::remove_file(&from)
            };
            removed.map_err(|error| format!("{path} not removed: {error}"))?;
            say(&format!(
                "{path} removed: an earlier one is already aside at {ASIDE}"
            ));
        } else {
            fs::DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(&aside)
                .map_err(|error| format!("{ASIDE}: {error}"))?;
            fs::rename(&from, aside.join(name))
                .map_err(|error| format!("{path} not moved aside: {error}"))?;
            say(&format!("{path} moved aside to {ASIDE}"));
        }
    }
    Ok(())
}

/// [`ASIDE`] removed, once the new OpenClaw runs: a link as a link.
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
        Ok(()) => say(&format!("the OpenClaw moved aside to {ASIDE} removed")),
        Err(error) => say(&format!("{ASIDE} not removed: {error}")),
    }
}

/// The folder fetched files are kept in while the install runs: under the
/// one-shot's private `/tmp`, root's, 0755 (so `lampos` reads them, never
/// changes them). Taken away when dropped.
struct Downloads {
    folder: PathBuf,
}

impl Downloads {
    fn make() -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let folder =
            std::env::temp_dir().join(format!("{DOWNLOADS}{}-{stamp}", std::process::id()));
        fs::DirBuilder::new().mode(0o755).create(&folder)?;
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755))?;
        Ok(Self { folder })
    }

    fn file(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }
}

impl Drop for Downloads {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

#[cfg(test)]
mod tests;
