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
//! lamp-tasks merges its models, agents and skills keys in at its start),
//! checked by OpenClaw's own `config validate` before it lands (#7868).
//!
//! **As `lampos`, in a namespace, as Hermes.** [`WRAPPER`] and [`NS`] are
//! Hermes's ([`crate::hermes_install::WRAPPER_TEXT`],
//! [`crate::hermes_install::ns_text`]) with OpenClaw's folder and command:
//! OpenClaw sees only [`ALLOWED`] (its own folder, the work folder, the
//! skills read-only) over the same empty read-only `/run`, `/var/lib` and
//! `/etc/lampOS`, its own `/tmp`, and no camera, mic or serial device. The
//! gateway runs the tools the model chooses, so it is the gateway that runs
//! in the namespace. As Hermes, OpenClaw has a PID namespace of its own,
//! but one for the Gateway and all its panes: OpenClaw 2026.11.2 refuses
//! to open its state from a PID namespace other than its Gateway's
//! (`GatewayLockNamespaceError`; with one per call every Harness pane
//! exited at once on lamp-c0a4). The gateway's unit ([`GATEWAY_ENV`] on)
//! starts it in a new user, mount and PID namespace ([`NS_FLAGS`]); every
//! other call joins the running gateway's, found by its unit's `MainPID`
//! (systemd's, not a file OpenClaw could write), with `nsenter`, and drops
//! back to the caller's ids in a nested user namespace as [`NS`] does. So
//! OpenClaw's `/proc` lists only OpenClaw's processes, and it can signal
//! none of the lamp's services though it runs as `lampos`, as they do.
//! With no gateway running, a call fails closed: it never runs in a new
//! namespace of its own nor in the lamp's. A restart of the gateway ends
//! its panes with its namespace. It is given only lamp-cloud's
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
    agent_ns, harness_install,
    hermes_install::{
        self, NEEDS_AVAILABLE_KB, RETRY_IN_NEXT_WINDOW, Shown, available_kb, free_kb, gid_of,
        holds, lock_named, on, owner, userns_argv,
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
/// [`SWITCH`] unset: on, beside Hermes under Harness (#6226; Hermes the
/// lamp's default engine, #6498),
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
/// The one file agent `main`'s exec allow-list names with the one run path
/// on (#6685, [`exec_allowlist`]): a skill's run on OpenClaw is a Harness
/// `new` OpenClaw turn (agent `main`), which execs this launcher by its
/// absolute path, never a file in [`WORK`] (which every gateway agent can
/// write, so nothing there is allowed). A byte copy of the installed
/// lamp-tasks ([`TASKS_BINARY`]), root's, 0755, beside lamp-update's own
/// fixed files ([`sync_launcher`]); it reaches lamp-tasks through
/// [`START_SOCKET_DIR`]. Taken out with the one run path off.
pub const SKILL_START: &str = "/usr/local/lib/lampos/skill-start";
/// The installed lamp-tasks [`SKILL_START`] is a copy of: lamp-tasks.service's
/// own `ExecStart` program (`hermes_install::RELAY`), its release's
/// `current` link followed.
pub const TASKS_BINARY: &str = hermes_install::RELAY;
/// The folder of lamp-tasks' socket for [`SKILL_START`] (`start.sock`,
/// which lamp-tasks makes in it). lamp-update's, never lamp-tasks' runtime
/// folder (which a lamp-tasks restart wipes, leaving the gateway's bind of
/// it stale): with the one run path on, the gateway's unit makes it at
/// every start and keeps it past a stop ([`unit_text_for`]: `lampos`'s,
/// 0755, until a reboot), Harness's one-shot makes it at once
/// ([`sync_socket_dir`]), and [`NS`] shows it read-only (a unix socket
/// still connects on a read-only mount), skipped when it is not there
/// ([`RUNS_EDITS`]). Off, taken out.
pub const START_SOCKET_DIR: &str = "/run/lampos-skill-start";
/// The agent [`SKILL_START`] is allowed for.
pub const RUN_AGENT: &str = "main";
/// The fence agent of a withdrawn design (#7101): its [`OLD_RUN_START`]
/// entry is taken out of OpenClaw's exec approvals.
pub const OLD_FENCE_AGENT: &str = "lamp-skill-run";
/// The glob an earlier build of #6685 allowed (a run's `start` in
/// [`WORK`], which every gateway agent can write): taken out of agent
/// [`RUN_AGENT`]'s and [`OLD_FENCE_AGENT`]'s allow-lists, never added.
pub const OLD_RUN_START: &str = "/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-*/start";
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
/// lampOS's own CDN (#6414; 58030966 bytes, root `node-v24.22.2-linux-arm64`),
/// byte-for-byte nodejs.org's ([`NODE_ORIGIN`]). Only its path is shared
/// with Harness: Harness's own Node is under lampos-tasks' 0700 home.
pub const NODE_URL: &str =
    "https://cdn.autonomous.ai/harness/runtime/node/v24.22.2/node-v24.22.2-linux-arm64.tar.gz";
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
/// The flags of the first `unshare` (the gateway's and the install's): a
/// user, mount and PID namespace of its own, with its own `/proc`, the
/// `unshare` its parent (`--kill-child`: killing it kills OpenClaw).
pub const NS_FLAGS: [&str; 8] = [
    "--map-root-user",
    "--mount",
    "--pid",
    "--fork",
    "--kill-child",
    "--mount-proc",
    "--propagation",
    "private",
];
/// Set to `on` by the gateway's unit only ([`UNIT_TEXT`]): [`WRAPPER`]
/// then makes the namespaces ([`NS_FLAGS`]); without it, a call joins the
/// running gateway's. Taken out of OpenClaw's environment either way.
pub const GATEWAY_ENV: &str = "LAMP_OPENCLAW_GATEWAY";
/// The hard bound of each of OpenClaw's long steps ([`runs_long_step`]:
/// a fetch, Node's unpacking, npm's install), past which the step is
/// killed and the install recorded `failed` with `timeout` (#6226: on
/// lamp-c0a4 an npm install with no output flags hung 38 min at its
/// extract; with `--foreground-scripts --loglevel=http` it took 3 min).
/// The clock goes on while the watch has the one-shot frozen, so the
/// bound a step is given is [`LONG_STEP_WITHIN`].
pub const STEP_WITHIN: Duration = Duration::from_secs(20 * 60);
/// The bound each long step is given ([`crate::install::bound_of`]):
/// [`STEP_WITHIN`] running and the most the one-shot may be kept frozen
/// ([`crate::hermes_schedule::MOST_FROZEN`]), as Hermes's stage has
/// (`HERMES_STAGE_WITHIN`). A step frozen past 20 min, for talk or a hot
/// board (#7879), is not taken for timed out on its thaw; frozen 40 min
/// in all, the watch stops it as `interrupted`, tried again next round.
pub const LONG_STEP_WITHIN: Duration =
    Duration::from_secs(STEP_WITHIN.as_secs() + crate::hermes_schedule::MOST_FROZEN.as_secs());
/// The largest file fetched (`curl --max-filesize`).
const MOST_BYTES: &str = "536870912";
/// The prefix of the folder fetched files are kept in while the install
/// runs: a step that names it is a long one ([`runs_long_step`]).
const DOWNLOADS: &str = "lamp-update-openclaw-";

/// [`WRAPPER`]: Hermes's wrapper with OpenClaw's state folder and the
/// pinned Node first on the `PATH`. Reads the token and fails closed
/// without it. Run by the gateway's unit ([`GATEWAY_ENV`] on), it runs
/// [`NS`] in a new user, mount and PID namespace ([`NS_FLAGS`]); run by
/// anyone else (a Harness pane), it joins the running gateway's
/// namespaces, found by the unit's `MainPID`, and runs OpenClaw there as
/// the caller's ids, or fails closed with no gateway running.
pub const WRAPPER_TEXT: &str = r#"#!/bin/sh
# lampOS: OpenClaw on the lamp (#6226; lampOS/docs/install.md, "OpenClaw on the lamp"). Written by lamp-update.
# OpenClaw (its gateway and each Harness pane) runs as its caller (lampos), in a user and mount namespace where it sees
# only its own folder, its work folder and the skills, no lamp service's files or sockets and no camera, mic or serial
# device; it is given only lamp-cloud's local forwarder token. Its gateway's unit (LAMP_OPENCLAW_GATEWAY=on) starts it in
# a PID namespace of its own, and every other call joins the running gateway's namespaces, so OpenClaw sees and can
# signal only OpenClaw's processes, never a lamp service; with no gateway running, a call fails. It is never run any
# other way.
T=$(cat /var/lib/lampos-cloud/hermes-token 2>/dev/null) && [ -n "$T" ] || {
  echo "openclaw: no forwarder token at /var/lib/lampos-cloud/hermes-token; lamp-cloud makes it with LAMP_OPENCLAW_ON_LAMP=on" >&2
  exit 1
}
LAMP_NS_UID=$(id -u) && LAMP_NS_GID=$(id -g) || exit 1
export LAMP_HERMES_TOKEN="$T" OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state OPENCLAW_NO_AUTO_UPDATE=1 PATH="/var/lib/lampos-openclaw/node/bin:$PATH" LAMP_NS_UID LAMP_NS_GID
unset T
A=${LAMP_OPENCLAW_GATEWAY-}
unset LAMP_OPENCLAW_GATEWAY
if [ "$A" = on ]; then
  exec /usr/bin/unshare --map-root-user --mount --pid --fork --kill-child --mount-proc --propagation private /usr/local/lib/lampos/openclaw-ns "$@"
fi
# The gateway's namespaces, by its unit's main process (that unshare, as lampos, in a user namespace of its own, still in
# the unit's cgroup, so a pid reused after the gateway stopped is never taken for it). LAMP_OPENCLAW_GATEWAY=on from any
# other caller only gives that caller namespaces of its own, never a way into the lamp's.
P=$(/usr/bin/systemctl show -p MainPID --value lamp-openclaw-gateway.service 2>/dev/null) || P=
case $P in '' | 0 | *[!0-9]*) P= ;; esac
if [ -n "$P" ] && { [ "$(stat -c %u "/proc/$P" 2>/dev/null)" != "$LAMP_NS_UID" ] || [ "$(readlink "/proc/$P/ns/user")" = "$(readlink /proc/self/ns/user)" ] || ! grep -q '/lamp-openclaw-gateway\.service$' "/proc/$P/cgroup" 2>/dev/null; }; then P=; fi
[ -n "$P" ] || {
  echo "openclaw: its gateway (lamp-openclaw-gateway.service) is not running in its own namespaces; nothing runs without it" >&2
  exit 1
}
export HOME=/var/lib/lampos-openclaw
exec /usr/bin/nsenter --user="/proc/$P/ns/user" --mount="/proc/$P/ns/mnt" --pid="/proc/$P/ns/pid_for_children" --preserve-credentials --wd="$PWD" -- /usr/bin/unshare --user --map-user="$LAMP_NS_UID" --map-group="$LAMP_NS_GID" /var/lib/lampos-openclaw/npm/node_modules/.bin/openclaw "$@"
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

/// (#8289) [`NS`]'s gateway mode, after [`NPM_CI_MODE`] and before
/// [`RUN_LINE`]: for the unit's `gateway run` only, OpenClaw is started as
/// a child, and this shell stays the PID namespace's PID 1. The kernel
/// drops a signal from outside a PID namespace to its PID 1 when PID 1 has
/// no handler for it; with the gateway exec'd as PID 1, Node had none for
/// SIGTERM until the gateway had loaded (tens of seconds on a lamp), so
/// systemd's one SIGTERM of a stop soon after a start was lost and the
/// stop waited its whole `TimeoutStopUSec` (90 s) and ended `failed`
/// (lamp-52e6, 22 Nov, twice). The shell traps SIGTERM before OpenClaw
/// starts and passes it on (a child that has no handler yet just ends),
/// waits for OpenClaw and exits with its status, 0 when it ended on the
/// SIGTERM passed on (143), as systemd takes a SIGTERM for a clean stop.
/// The unit's `MainPID` is still the `unshare` outside, so a pane's way in
/// (`pid_for_children`) is the same namespace. Every other call runs
/// OpenClaw by [`RUN_LINE`], as before.
pub const GATEWAY_RUN: &str = "# (#8289) The gateway runs as this shell's child, not as the PID namespace's PID 1, which drops a signal from
# outside that it has no handler for (Node has none for SIGTERM until the gateway has loaded): this shell passes a
# stop's SIGTERM on and exits as the gateway does, 0 when it ended on that SIGTERM.
if [ \"${1-}\" = gateway ] && [ \"${2-}\" = run ]; then
  t=
  trap 't=TERM' TERM
  /usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" /var/lib/lampos-openclaw/npm/node_modules/.bin/openclaw \"$@\" &
  c=$!
  trap 't=TERM; kill -TERM \"$c\" 2>/dev/null' TERM
  [ -z \"$t\" ] || kill -TERM \"$c\" 2>/dev/null
  while wait \"$c\"; s=$?; [ \"$s\" -gt 128 ] && kill -0 \"$c\" 2>/dev/null; do :; done
  if [ \"$s\" -gt 128 ]; then wait \"$c\"; r=$?; [ \"$r\" = 127 ] || s=$r; fi
  if [ \"$s\" = 143 ] && [ -n \"$t\" ]; then s=0; fi
  exit \"$s\"
fi
";

/// What [`ns_text`] changes with [`Shown::one_path`] and [`Shown::tasks`]
/// on (the gateway mark's, [`wrapper_text_for`]), after [`ENGINE_EDITS`],
/// each found exactly once (a test holds it): the pushed skills' folder
/// (`hermes_install::PUSHED`) is shown read-only whenever it is there, not
/// only with `LAMP_NS_NET=run`, which the gateway's unit never sets: a
/// skill's run, one turn on the gateway, runs a pushed skill's script from
/// it. And [`START_SOCKET_DIR`], [`SKILL_START`]'s way to lamp-tasks:
/// staged with the rest before `/run` is hidden, moved back and remounted
/// read-only, and its staging folder removed; skipped when it is not
/// there, as the pushed skills are. Nothing else of
/// `LAMP_NS_NET`'s (no network namespace, no `model.sock`, no relay)
/// comes with it.
const RUNS_EDITS: [(&str, &str); 6] = [
    (
        "# It also shows the skills the owner's computer pushed, /var/lib/lampOS/skills-dev, read-only, when there.\n",
        "# It also shows the skills the owner's computer pushed, /var/lib/lampOS/skills-dev, read-only, when there, for the\n\
         # gateway's every turn (#6685: a skill's run is one turn on it, its script under that folder), and\n\
         # /run/lampos-skill-start, read-only, when there: /usr/local/lib/lampos/skill-start's way to lamp-tasks.\n",
    ),
    (
        "if [ \"${LAMP_NS_NET-}\" = run ] && [ -d \"$P\" ]; then mkdir -m 0700",
        "if [ -d \"$P\" ]; then mkdir -m 0700",
    ),
    (
        " P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns\n",
        // Q= on a line of its own after the folders' line, so that line
        // stays byte for byte the one lamp-tasks reads as the namespace
        // showing lamp-control's socket (`lamp_tasks::skill_task::
        // AGENT_NS_TASKS_LINE`; c0a4's #7133 dry run, 15 Nov).
        " P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns\nQ=/run/lampos-skill-start\n",
    ),
    (
        "|| fail \"no $P\"; else P=; fi\n",
        "|| fail \"no $P\"; else P=; fi\n\
         if [ -d \"$Q\" ]; then mkdir -m 0700 \"$S/q\" && mount --bind \"$Q\" \"$S/q\" || fail \"no $Q\"; else Q=; fi\n",
    ),
    (
        "&& ro \"$P\" || fail \"$P not shown\"; fi\n",
        "&& ro \"$P\" || fail \"$P not shown\"; fi\n\
         if [ -n \"$Q\" ]; then mkdir -p \"$Q\" && mount --move \"$S/q\" \"$Q\" && ro \"$Q\" || fail \"$Q not shown\"; fi\n",
    ),
    (
        "{ [ -z \"$P\" ] || rmdir \"$S/p\"; } &&",
        "{ [ -z \"$P\" ] || rmdir \"$S/p\"; } && { [ -z \"$Q\" ] || rmdir \"$S/q\"; } &&",
    ),
];

/// What a round shows OpenClaw (#6882, #6685): `skills` (its skills
/// switch) as read; lamp-control's socket, the pushed skills and the
/// gateway mark ([`Shown::tasks`] and [`Shown::one_path`]) with the one
/// run path on (`one_path`: `LAMP_SKILL_DEFAULT_AGENT=on` with
/// `LAMP_SKILL_TASKS` on), the same switch Hermes's one run path wrapper
/// follows, and nothing else: lamp-update reads no gateway runs switch of
/// its own (the lead on #6685: a skill's run on OpenClaw goes through
/// Harness `new` as on Hermes, lamp-tasks deciding). With it off, OpenClaw
/// is shown what it was before #6685: [`ns_text`] and [`wrapper_text_for`]
/// byte for byte as before.
pub fn shown(skills: bool, one_path: bool) -> Shown {
    let runs = one_path;
    Shown {
        skills,
        tasks: runs,
        one_path: runs,
    }
}

/// A long-lived gateway must not keep a mount view made before control's
/// socket exists. Type=simple ordering alone only waits for its process.
/// Not there after 60 s, it logs one line and starts without it, never
/// failing: a lamp before setup has no lamp-control (its unit's
/// ConditionPathExists), and a failed start would also run into the
/// unit's start limit, keeping the gateway down after setup too.
/// The wrapper clears GATEWAY_ENV before exec, so use the unit's exact
/// `gateway run` arguments. Install/version calls keep their old behaviour.
pub(crate) const WAIT_FOR_CONTROL: &str = r#"# Wait before the gateway captures control's socket in its mount view.
if [ "${1-}" = gateway ] && [ "${2-}" = run ]; then
  control_wait=0
  while [ ! -S "$L" ]; do
    if [ "$control_wait" -ge 60 ]; then
      echo "lampos ns: lamp-control's socket not there after 60 s; the gateway starts without it" >&2
      break
    fi
    sleep 1 || { echo "lampos ns: lamp-control's socket wait interrupted; the gateway starts without it" >&2; break; }
    control_wait=$((control_wait + 1))
  done
  unset control_wait
fi
# The bind below shows the socket when it is there; else L= as before.
"#;

/// [`NS`]'s text: Hermes's ([`hermes_install::ns_text`]`(skills, false)`,
/// the same allow-list, the same steps) for OpenClaw ([`ENGINE_EDITS`]),
/// with its install mode ([`NPM_CI_MODE`]) and its gateway mode
/// ([`GATEWAY_RUN`], #8289) before OpenClaw runs. With
/// `shown.skills`, lamp-skills' socket folder is shown read-only too; with
/// `shown.tasks`, lamp-control's socket alone, read-only (a round asks for
/// it with the one run path on only: a skill's run on the gateway reaches
/// the lamp through it, #6685 step 4); neither, as before. With both
/// `shown.tasks` and `shown.one_path`, the pushed skills' folder too
/// ([`RUNS_EDITS`]); otherwise byte for byte as before.
/// A voice make's home lines ([`hermes_install::NS_HOME_LINE`]) are
/// Hermes's alone: a make runs Hermes only, so they are taken out.
pub fn ns_text(shown: Shown) -> String {
    let mut text = hermes_install::ns_text(shown.skills, shown.tasks);
    if shown.tasks {
        text = text.replacen(hermes_install::NS_HOME_LINE, "", 1);
    }
    for (from, to) in ENGINE_EDITS {
        text = text.replace(from, to);
    }
    if shown.tasks && shown.one_path {
        for (from, to) in RUNS_EDITS {
            text = text.replacen(from, to, 1);
        }
        let bind = "if [ -S \"$L\" ]; then";
        text = text.replacen(bind, &format!("{WAIT_FOR_CONTROL}{bind}"), 1);
    }
    text.replacen(
        RUN_LINE,
        &format!("{NPM_CI_MODE}{GATEWAY_RUN}{RUN_LINE}"),
        1,
    )
}

/// [`WRAPPER`]'s text: [`WRAPPER_TEXT`] with the shared namespace piece's
/// journal line ([`agent_ns::with_hook_note`], #6530) and its `PATH`
/// before the `nsenter` into the running gateway's namespaces
/// ([`agent_ns::with_ns_path`]), which a pane's OpenClaw runs by without
/// [`NS`]; the gateway's own start gets it from [`ns_text`].
pub fn wrapper_text() -> String {
    agent_ns::with_ns_path(&agent_ns::with_hook_note(WRAPPER_TEXT, "openclaw"))
}

/// The line after `#!/bin/sh` of OpenClaw's wrapper with the one run path
/// on ([`Shown::one_path`], #6685 step 4): the same for every gateway
/// agent, naming none, what lamp-tasks looks for
/// (`lamp_tasks::gateway::GATEWAY_MARK`) before a skill's run goes to
/// the lamp's default agent as one turn on its warm gateway.
pub const GATEWAY_MARK: &str = "# lampOS: a gateway agent: a skill's run is one turn on its warm gateway (#6685). Written by lamp-update.\n";

/// OpenClaw's data as a gateway agent, after [`GATEWAY_MARK`]: shell
/// comments, each `#@<key> <value>`, which lamp-tasks reads
/// (`lamp_tasks::gateway::GatewayAgent`) and no shell runs. A skill's run
/// on OpenClaw is a Harness `new` OpenClaw turn (agent `main`), which
/// execs [`SKILL_START`] by absolute path, the one file agent `main`'s
/// exec allow-list names ([`exec_allowlist`]).
/// The `#@agent`, `#@turn`, `#@stop`, `#@end` and `#@fence-*` keys are
/// from the withdrawn fence-turn design (#7101): nothing uses them after
/// #7133, but `GatewayAgent`'s parse still needs every key, and they stay
/// for the wrapper's byte identity. Never change these bytes.
pub const GATEWAY_DATA: &str = r#"#@port 18789
#@ns /usr/local/lib/lampos/openclaw-ns
#@agent lamp-skill-run
#@turn gateway call agent --expect-final --json --timeout {ms} --params {"agentId":"{agent}","sessionKey":"agent:{agent}:{name}","message":{message},"idempotencyKey":"{name}","timeout":{secs},"deliver":false}
#@stop gateway call chat.abort --json --timeout {ms} --params {"sessionKey":"agent:{agent}:{name}"}
#@end gateway call sessions.delete --json --timeout {ms} --params {"key":"agent:{agent}:{name}","deleteTranscript":true}
#@fence-get approvals get --gateway --json --timeout {ms}
#@fence-set approvals set --gateway --stdin --timeout {ms}
#@fence-entry {"security":"allowlist","ask":"off","askFallback":"deny","autoAllowSkills":false,"allowlist":[{"pattern":"{start}"}]}
"#;

/// [`WRAPPER`]'s text for `shown`: with [`Shown::one_path`], OpenClaw a
/// gateway agent ([`GATEWAY_MARK`] and [`GATEWAY_DATA`] after `#!/bin/sh`,
/// the rest [`wrapper_text`] as it is); else [`wrapper_text`], byte for
/// byte.
pub fn wrapper_text_for(shown: Shown) -> String {
    let text = wrapper_text();
    if !shown.one_path {
        return text;
    }
    let rest = text.strip_prefix("#!/bin/sh\n").unwrap_or(&text);
    format!("#!/bin/sh\n{GATEWAY_MARK}{GATEWAY_DATA}{rest}")
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
/// again whenever it exits, given up on after five starts in ten minutes.
/// `Restart=always`, not `on-failure` (#8238): OpenClaw answers a change of
/// its `gateway` keys with a "full process restart (supervisor restart)"
/// that exits 0, and leaves the start to its supervisor; `on-failure` took
/// that as done and left the gateway down. A `systemctl stop` (every stop
/// lamp-update and lamp-setup make) still stops it.
/// `TimeoutStopSec=150` (#8320): a stop that lands in the gateway's first
/// start (a `PartOf=` restart by lamp-control, which [`settled`] never
/// sees) drains about 80 s (79.9 s on kit-8306b); the default 90 s SIGKILLed
/// one and left OpenClaw's state unreadable (gate G), so it gets 150 s.
pub const UNIT_TEXT: &str = "\
# lamp-openclaw-gateway: OpenClaw's gateway on the lamp, which Harness's OpenClaw panes talk to (lampOS/docs/install.md,
# \"OpenClaw on the lamp\", #6226). Written by a round of lamp-update with LAMP_OPENCLAW_ON_LAMP on, enabled and always
# on while it is (the owner: every lamp has 6 GB), disabled and stopped with it off. It runs only through
# /usr/local/bin/openclaw, in the namespaces that wrapper makes for it (LAMP_OPENCLAW_GATEWAY=on: its own PID namespace,
# which Harness's panes join), as lampos, on loopback port 18789 with its own token.
[Unit]
Description=lampOS OpenClaw gateway for Harness (lamp-openclaw-gateway)
After=network-online.target lamp-cloud.service
StartLimitIntervalSec=600
StartLimitBurst=5

[Service]
Type=simple
User=lampos
Group=lampos
Environment=HOME=/var/lib/lampos-openclaw OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state LAMP_OPENCLAW_GATEWAY=on
WorkingDirectory=/var/lib/lampos-openclaw/state
ExecStart=/usr/local/bin/openclaw gateway run
Restart=always
RestartSec=10
# (#8320) a stop in the gateway's first start drains about 80 s: room for it before a SIGKILL.
TimeoutStopSec=150
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

/// The lines [`unit_text_for`] adds after [`UNIT_TEXT`]'s
/// `WorkingDirectory=` with the one run path on: [`START_SOCKET_DIR`] made
/// by systemd at each start, `lampos`'s, 0755, and kept past a stop or
/// restart (until a reboot), so lamp-tasks' socket in it outlives a
/// gateway restart and the folder is there before [`NS`] shows it.
pub const START_DIR_LINES: &str = "\
# LAMP_SKILL_DEFAULT_AGENT=on (#6685): the folder of lamp-tasks' socket for /usr/local/lib/lampos/skill-start, which
# openclaw-ns shows read-only; kept past a stop, so lamp-tasks' socket in it outlives a gateway restart.
RuntimeDirectory=lampos-skill-start
RuntimeDirectoryMode=0755
RuntimeDirectoryPreserve=yes
";

/// The lines [`unit_text_for`] puts for [`UNIT_TEXT`]'s `After=` with
/// [`Shown::tasks`] and [`Shown::one_path`] on (#7135, #8320).
pub const CONTROL_LINES: &str = "After=network-online.target lamp-cloud.service lamp-control.service\n\
    Wants=lamp-control.service\n\
    PartOf=lamp-control.service\n";

/// The line of [`CONTROL_LINES`] that restarts the gateway with
/// lamp-control (#8320).
const PART_OF: &str = "PartOf=lamp-control.service";

/// [`UNIT`]'s text for `shown`: with [`Shown::one_path`], [`UNIT_TEXT`]
/// with [`START_DIR_LINES`] after its `WorkingDirectory=`; else
/// [`UNIT_TEXT`], byte for byte. With [`Shown::tasks`] too, it starts
/// after and wants lamp-control, and is `PartOf=` it (#8320): its
/// namespace binds lamp-control's socket file once, at its start, and
/// lamp-control's `RuntimeDirectory` is made anew (a new socket inode) at
/// each lamp-control start, so a lamp-control restart (a round's switch,
/// `lamp-setup restart`, the keeper, its own `Restart=on-failure`) restarts
/// the gateway too, and a stop stops it (a round's `gateway_on` starts it
/// again). The cost: the gateway's start, about 60 s, after each
/// lamp-control restart.
pub fn unit_text_for(shown: Shown) -> String {
    let anchor = "WorkingDirectory=/var/lib/lampos-openclaw/state\n";
    if !shown.one_path {
        return UNIT_TEXT.to_owned();
    }
    let text = UNIT_TEXT.replacen(anchor, &format!("{anchor}{START_DIR_LINES}"), 1);
    if shown.tasks {
        text.replacen(
            "After=network-online.target lamp-cloud.service\n",
            CONTROL_LINES,
            1,
        )
    } else {
        text
    }
}

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
    /// With `installed`: agent `main`'s [`SKILL_START`] allow-list entry
    /// was added to OpenClaw's exec approvals ([`exec_allowlist`]) by this
    /// build, with the one run path on. False until an add succeeded, so a
    /// round asks again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exec_allowlist: bool,
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
            exec_allowlist: false,
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
    /// [`Shown::tasks`]: lamp-control's socket shown (a round asks for it
    /// with the one run path on only, #6685 step 4).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    tasks: bool,
    /// [`Shown::one_path`]: OpenClaw's wrapper a gateway agent's
    /// ([`wrapper_text_for`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    one_path: bool,
    /// [`BY_DEFAULT_SWITCH`] on in the round that asked: the one-shot then
    /// starts or restarts the gateway only while OpenClaw is the lamp's
    /// default agent, as lamp-tasks says it right before ([`steps`]). Off
    /// (an ask of an earlier build, the image's pre-install): always, as
    /// before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    by_default: bool,
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
/// leaves OpenClaw's ask to that one-shot, none started for it.
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
pub fn present(root: &Path, shown: Shown) -> bool {
    fs::metadata(on(root, OPENCLAW)).is_ok()
        && package_version(root).as_deref() == Some(VERSION)
        && holds(root, LOCKFILE_AT, LOCKFILE)
        && node_version(root).as_deref() == Some(NODE_VERSION)
        && holds(root, WRAPPER, &wrapper_text_for(shown))
        && holds(root, NS, &ns_text(shown))
        && agent_ns::hook_shim_there(root)
        && fs::read(unit_path(root)).is_ok_and(|bytes| bytes == unit_text_for(shown).as_bytes())
}

/// When `serve` starts, as Hermes's: off, an earlier record and ask are
/// taken away; on with OpenClaw missing and no record, `absent`.
pub fn at_start(root: &Path, records: &Path, switch: bool, shown: Shown, now_unix_ms: u64) {
    if !switch {
        for name in [RECORD, ASK] {
            let _ = fs::remove_file(records.join(name));
        }
        return;
    }
    if !present(root, shown) && record(records).is_none() {
        write(records, OpenclawInstall::new("absent", None, now_unix_ms));
    }
}

/// An install's ask for `shown`, as a round writes it: the image's
/// pre-install (crate::preinstall) writes the same one.
pub fn ask_install(records: &Path, shown: Shown) -> io::Result<()> {
    ask(records, false, shown)
}

fn ask(records: &Path, remove: bool, shown: Shown) -> io::Result<()> {
    ask_with(records, remove, shown, false)
}

/// [`ask`], with [`Ask::by_default`].
fn ask_with(records: &Path, remove: bool, shown: Shown, by_default: bool) -> io::Result<()> {
    let text = serde_json::to_vec(&Ask {
        version: 1,
        remove,
        skills: shown.skills,
        tasks: shown.tasks,
        one_path: shown.one_path,
        by_default,
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
/// systemd reloaded when it changed, and with Hermes's install under way
/// only the ask is written: the one-shot installs OpenClaw after Hermes.
/// With the one run path on and agent `main`'s entry recorded, the round
/// adds it again itself through `run` ([`keep_run_entry`]): a factory
/// reset wipes OpenClaw's approvals but keeps the record.
#[allow(clippy::too_many_arguments)]
pub fn after_round(
    root: &Path,
    records: &Path,
    shown: Shown,
    systemd: &mut dyn Systemd,
    run: &mut Runner,
    now_unix_ms: u64,
    window: Result<crate::hermes_schedule::Window, String>,
    say: &mut dyn FnMut(&str),
) -> Option<OpenclawInstall> {
    after_round_with(
        root,
        records,
        shown,
        Gateway::Wanted,
        false,
        systemd,
        run,
        now_unix_ms,
        window,
        say,
    )
}

/// [`after_round`], with whether the gateway is wanted now ([`Gateway`],
/// #8105): wanted, it is enabled and started as before; not wanted, it is
/// stopped and not started at boot, and agent `main`'s allow-list entry,
/// which goes through the running gateway, waits for a round that wants
/// it; unknown, it is left as it is. `by_default`: [`BY_DEFAULT_SWITCH`]
/// on, handed to the one-shot in its ask ([`Ask::by_default`]), so it
/// starts the gateway only while OpenClaw is the default. With the other
/// engine's gateway on too, main stops the unwanted gateway before this
/// runs and passes a wanted one as unknown while the other still runs
/// ([`crate::gateway_keeper::round_gateways`]).
#[allow(clippy::too_many_arguments)]
pub fn after_round_with(
    root: &Path,
    records: &Path,
    shown: Shown,
    gateway: Gateway,
    by_default: bool,
    systemd: &mut dyn Systemd,
    run: &mut Runner,
    now_unix_ms: u64,
    window: Result<crate::hermes_schedule::Window, String>,
    say: &mut dyn FnMut(&str),
) -> Option<OpenclawInstall> {
    // #8320: whether the unit there before this write already restarted the
    // gateway with lamp-control (`PartOf=`), read before the write.
    let had_part_of = fs::read_to_string(root.join(crate::writer_unit::FOLDER).join(UNIT))
        .is_ok_and(|text| text.lines().any(|line| line == PART_OF));
    match crate::writer_unit::write_unit(root, UNIT, &unit_text_for(shown)) {
        Ok(written) => {
            let reloaded = match written.then(|| systemd.reload()) {
                Some(Err(why)) => {
                    say(&format!("{UNIT} written, but systemd not reloaded: {why}"));
                    false
                }
                _ => written,
            };
            let restarted = loaded_as_written(systemd, say);
            // #8320: a gateway showing lamp-control's socket, its unit just
            // written anew with `PartOf=` over one without it (the round
            // that ships `PartOf=`, which restarted lamp-control before this
            // write), is try-restarted once, so its namespace binds the
            // socket lamp-control has now. Not when the unit before had
            // `PartOf=` too: a lamp-control restart in this round's switch
            // has restarted the gateway seconds ago, and a second stop in
            // its first start is #8324's 80 s drain (gate G). Only on a
            // write (the next round finds the same bytes) and only a
            // running one (a `failed` one is left to `gateway_on`).
            if reloaded && !restarted && !had_part_of && shown.tasks && shown.one_path {
                match systemd.try_restart_gateway() {
                    Ok(()) => say(&format!(
                        "{UNIT} written anew: the gateway restarted, so it shows lamp-control's socket as it is now (#8320)"
                    )),
                    Err(why) => say(&format!(
                        "{UNIT} written anew, the gateway not restarted: {why} (#8320)"
                    )),
                }
            }
        }
        Err(why) => say(&format!("OpenClaw's gateway unit not written: {why}")),
    }
    let mut before = record(records);
    // OpenClaw there, but agent main's allow-list entry not added yet with
    // the one run path on (an earlier build's, or an add that failed), or
    // still recorded with it off, or the launcher not this lamp-tasks'
    // copy: the one-shot is asked (its steps do only that, the gateway
    // running), and asked again each round until done.
    let added = |said: &OpenclawInstall| said.install == "installed" && said.exec_allowlist;
    let mut allowlist_due = if shown.one_path {
        !before.as_ref().is_some_and(added)
    } else {
        before.as_ref().is_some_and(added)
    };
    let there = present(root, shown);
    if there {
        keep_gateway(root, systemd, gateway, true, say);
    }
    // #6685: the entry recorded is added again every round with the
    // gateway up (an add of an entry there writes nothing), as a factory
    // reset wipes OpenClaw's approvals and keeps the record. One not taken
    // marks the record and asks the one-shot, which runs every edit.
    if there
        && shown.one_path
        && !allowlist_due
        && gateway != Gateway::NotWanted
        && keep_run_entry(run, say) == Some(false)
        && let Some(mut said) = before.take()
    {
        said.exec_allowlist = false;
        before = Some(write(records, said));
        allowlist_due = true;
    }
    // A gateway not wanted (#8105) asks no one-shot for the allow-list,
    // which goes through the running gateway: it would start it every round
    // only to be stopped again. The entry is added by a round that wants it.
    // #7868: the gateway block is read here only; the one-shot writes it,
    // checked through the running gateway, and restarts the gateway on it:
    // asked only while the gateway may run, so the one-shot never works on
    // a gateway the other engine's has replaced. OpenClaw's own files (the
    // launcher) are still kept, so a switch back to OpenClaw is fast.
    let due = (allowlist_due && gateway != Gateway::NotWanted)
        || launcher_due(root, shown)
        || (there && gateway != Gateway::NotWanted && gateway_block_due(root));
    if there && !due {
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
        // A tap from the app (#6240) is a deliberate try again: a recent
        // failure does not hold it, as for Hermes.
        if said.install == "failed"
            && !matches!(window, Ok(crate::hermes_schedule::Window::Asked))
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
    let hermes_first = hermes_under_way(records, now_unix_ms);
    if !hermes_first {
        say(&format!("OpenClaw may be installed now: {}", window.name()));
    }
    if let Err(error) = ask_with(records, false, shown, by_default) {
        say(&format!(
            "OpenClaw is not installed: {ASK} not written: {error}"
        ));
        return Some(write(records, failed(now_unix_ms, "not_started")));
    }
    // #7019: the one-shot running Hermes's reads this ask after Hermes's
    // install (`install_now`), so OpenClaw's follows in it; none is started.
    if hermes_first {
        say("OpenClaw's ask is written: it runs after Hermes in Harness's one-shot");
        return None;
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
    // The entry's state goes with the ask (#6685): a switch-off's removes
    // not run yet are still due in the one-shot ([`steps`]).
    write(
        records,
        OpenclawInstall {
            exec_allowlist: before.as_ref().is_some_and(|said| said.exec_allowlist),
            ..OpenclawInstall::new("installing", None, now_unix_ms)
        },
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

/// The switch (#8105): `on` exactly, in lamp-update's environment or the
/// carried `/etc/lampOS/update.env`. With it on, OpenClaw's gateway runs
/// only while OpenClaw is the lamp's default agent, as lamp-tasks says it
/// ([`said_default`]); off, it is always on with OpenClaw there, as before.
pub const BY_DEFAULT_SWITCH: &str = "LAMP_OPENCLAW_GATEWAY_BY_DEFAULT";

/// lamp-tasks' socket for the app's calls, where it says the lamp's default
/// agent (`app_engine_default_state`, the line lamp-cloud asks for
/// `update.status`).
pub const TASKS_APP_SOCKET: &str = "/run/lampos-tasks/app.sock";

/// How long lamp-tasks is given to answer: as lamp-cloud's wait.
pub const TASKS_WAIT: Duration = Duration::from_secs(2);

/// How often `serve` asks lamp-tasks, between rounds and during them, so a
/// choice of OpenClaw in the app starts its gateway within seconds.
pub const KEEP_EVERY: Duration = Duration::from_secs(10);

/// The most bytes of lamp-tasks' answer read: one small JSON line.
const MOST_TASKS_REPLY: u64 = 4096;

/// Whether OpenClaw's gateway is wanted now (#8105).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gateway {
    /// Enabled at boot and running: the switch off (always on, as before),
    /// or OpenClaw the lamp's default agent.
    Wanted,
    /// Stopped and not started at boot: another agent is the lamp's
    /// default.
    NotWanted,
    /// Left as it is: lamp-tasks not reached, or no default said yet.
    Unknown,
}

/// What lamp-tasks says on its read for lamp-update (#8105): the lamp's
/// default engine (`None` when it has none). Any other field of its answer
/// (an older lamp-tasks' `openclaw_in_use`, say) is not read: the gateways
/// follow the default only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Said {
    pub default: Option<String>,
}

/// lamp-tasks' answer at `socket`, `None` when it does not answer (down,
/// starting, its socket not there, or a refusal).
pub fn said_default(socket: &Path) -> Option<Said> {
    use std::io::{BufRead, BufReader, Read, Write};
    let asked = || -> io::Result<String> {
        let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(TASKS_WAIT))?;
        stream.set_write_timeout(Some(TASKS_WAIT))?;
        stream.write_all(b"{\"op\":\"app_engine_default_state\"}\n")?;
        let mut reply = String::new();
        BufReader::new(stream.take(MOST_TASKS_REPLY)).read_line(&mut reply)?;
        Ok(reply)
    };
    default_of(&asked().ok()?)
}

/// lamp-tasks' answer read ([`said_default`]): `None` for a refusal or a
/// line that is not its answer.
fn default_of(reply: &str) -> Option<Said> {
    let reply: serde_json::Value = serde_json::from_str(reply.trim()).ok()?;
    if reply.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return None;
    }
    Some(Said {
        default: reply
            .get("default")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|engine| !engine.is_empty())
            .map(str::to_owned),
    })
}

/// The rule (#8105): with the switch off, always [`Gateway::Wanted`]. On:
/// wanted while lamp-tasks says OpenClaw is the lamp's default agent, not
/// wanted while it says another, unknown with no answer or no default
/// said, so a guess never starts or stops it. The default only: one agent
/// gateway runs at a time (the owner's rule), so no hold keeps OpenClaw's
/// up after a switch away (#8105's 4 h `openclaw_in_use` hold is gone).
pub fn gateway_wanted(by_default: bool, said: Option<Said>) -> Gateway {
    if !by_default {
        return Gateway::Wanted;
    }
    match said.and_then(|said| said.default).as_deref() {
        Some("openclaw") => Gateway::Wanted,
        Some(_) => Gateway::NotWanted,
        None => Gateway::Unknown,
    }
}

/// The gateway kept as [`Gateway`] says, with OpenClaw there: wanted,
/// [`gateway_on`] (`round`: a round's look, the only one that resets a
/// `failed` gateway); not wanted, [`gateway_idle`]; unknown, nothing.
pub fn keep_gateway(
    root: &Path,
    systemd: &mut dyn Systemd,
    gateway: Gateway,
    round: bool,
    say: &mut dyn FnMut(&str),
) {
    match gateway {
        Gateway::Wanted => gateway_on(root, systemd, round, say),
        Gateway::NotWanted => gateway_idle(root, systemd, say),
        Gateway::Unknown => {}
    }
}

/// Between rounds (#8105), one look of [`crate::gateway_keeper`]'s: with
/// OpenClaw there ([`WRAPPER`] and its unit), the gateway kept as
/// [`Gateway`] says; not wanted, it is disabled and stopped whether OpenClaw
/// is there or not, so a gateway left running never holds the other
/// engine's start. Each change is said once; a gateway already as wanted is
/// left with no word. While Harness's one-shot is at work (an install or its
/// steps run through the gateway), a wanted gateway is left to it, but an
/// unwanted one is still stopped and kept disabled: the one-shot never
/// starts one that isn't wanted (`steps`). A gateway started here with the
/// one run path on (`one_path`) gets agent `main`'s allow-list entry tried
/// once at once ([`run_entry_once`]), never waited for (Twelve's P2 on
/// #8116): whether the entry is still owed (the gateway not ready yet) is
/// returned (`Some(true)`; `Some(false)` taken, refused or not tried), or
/// `None` when nothing was started or stopped.
pub fn keep_between_rounds(
    root: &Path,
    systemd: &mut dyn Systemd,
    gateway: Gateway,
    one_path: bool,
    run: &mut Runner,
    say: &mut dyn FnMut(&str),
) -> Option<bool> {
    let there = fs::symlink_metadata(on(root, WRAPPER)).is_ok()
        && fs::symlink_metadata(unit_path(root)).is_ok();
    if !there && gateway != Gateway::NotWanted {
        return None;
    }
    if one_shot_at_work(systemd) && gateway != Gateway::NotWanted {
        return None;
    }
    // Already as wanted: nothing run, nothing said, every KEEP_EVERY.
    let enabled = fs::symlink_metadata(on(root, WANTS)).is_ok();
    let active = gateway_state(systemd);
    let up = active
        .as_deref()
        .is_some_and(|active| active == "active" || active == "activating");
    let down = active
        .as_deref()
        .is_some_and(|active| active == "inactive" || active == "failed");
    // A `failed` gateway (past its start limit) is left to the next round,
    // which resets it (#8238): a reset every look would undo the limit
    // that ends a crash loop.
    let failed = active.as_deref() == Some("failed");
    let settled = match gateway {
        Gateway::Wanted => enabled && (up || failed),
        Gateway::NotWanted => !enabled && down,
        Gateway::Unknown => true,
    };
    if settled {
        return None;
    }
    if gateway == Gateway::Wanted && !failed {
        say(&format!(
            "{UNIT} started: OpenClaw is the lamp's agent (#8105)"
        ));
    }
    keep_gateway(root, systemd, gateway, false, say);
    if gateway != Gateway::Wanted || !one_path {
        return Some(false);
    }
    match run_entry_once(run, say) {
        Some(false) => {
            say(&format!(
                "agent {RUN_AGENT}'s allow-list entry not taken after the gateway started; the next round asks the one-shot"
            ));
            Some(false)
        }
        Some(true) => Some(false),
        None => Some(true),
    }
}

/// Whether Harness's one-shot ([`harness_install::UNIT`]) is at work
/// (`active` or `activating`).
pub fn one_shot_at_work(systemd: &mut dyn Systemd) -> bool {
    systemd
        .states(&[harness_install::UNIT.to_owned()])
        .ok()
        .and_then(|states| states.into_iter().next())
        .is_some_and(|state| state.active == "active" || state.active == "activating")
}

/// Whether the gateway is down as systemd says it (`inactive` or
/// `failed`): a state not read, or any other (`active`, `activating`,
/// `deactivating`, `reloading`), is not down.
pub fn gateway_down(systemd: &mut dyn Systemd) -> bool {
    matches!(
        gateway_state(systemd).as_deref(),
        Some("inactive" | "failed")
    )
}

/// The gateway not wanted (#8105): [`WANTS`] taken out (systemd reloaded),
/// so boot doesn't start it, and stopped when it runs. Said once, when it
/// changes something.
fn gateway_idle(root: &Path, systemd: &mut dyn Systemd, say: &mut dyn FnMut(&str)) {
    let wants = on(root, WANTS);
    if fs::symlink_metadata(&wants).is_ok() {
        match fs::remove_file(&wants) {
            Ok(()) => {
                say(&format!(
                    "{UNIT} disabled: OpenClaw is not the lamp's agent, so its gateway is not started at boot (#8105)"
                ));
                if let Err(why) = systemd.reload() {
                    say(&format!("systemd not reloaded: {why}"));
                }
            }
            Err(error) => say(&format!("{UNIT} not disabled: {error}")),
        }
    }
    let running =
        gateway_state(systemd).is_some_and(|active| active != "inactive" && active != "failed");
    if running {
        match systemd.stop(UNIT) {
            Ok(()) => say(&format!(
                "{UNIT} stopped: OpenClaw is not the lamp's agent (#8105)"
            )),
            Err(why) => say(&format!("{UNIT} not stopped: {why}")),
        }
    }
}

/// The gateway always on with OpenClaw there (the owner on #6226): enabled
/// at boot by [`WANTS`] (made when missing, as `systemctl enable` does,
/// systemd then reloaded) and started when it is down (#8238): `inactive`
/// (an exit the unit took as done) or, on a round's look (`round`),
/// `failed` (past its start limit), whose `reset-failed` comes first and is
/// said. Only a round resets it, so `StartLimitBurst` still ends a crash
/// loop between rounds, one reset per round. Running, or a state systemd
/// doesn't say, it is left as it is: a down gateway whose state can't be
/// read waits for the next look.
fn gateway_on(root: &Path, systemd: &mut dyn Systemd, round: bool, say: &mut dyn FnMut(&str)) {
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
    match gateway_state(systemd).as_deref() {
        Some("inactive") => {}
        Some("failed") if round => match systemd.reset_failed(UNIT) {
            Ok(()) => say(&format!(
                "{UNIT} failed: reset-failed, so it can be started again (#8238)"
            )),
            Err(why) => say(&format!("{UNIT} failed, and not reset-failed: {why}")),
        },
        _ => return,
    }
    if let Err(why) = systemd.start_unit(UNIT) {
        say(&format!("{UNIT} not started: {why}"));
    }
}

/// The gateway's `ActiveState` as systemd says it, `None` when it can't.
fn gateway_state(systemd: &mut dyn Systemd) -> Option<String> {
    systemd
        .states(&[UNIT.to_owned()])
        .ok()
        .and_then(|states| states.into_iter().next())
        .map(|state| state.active)
}

/// The properties [`loaded_as_written`] reads of [`UNIT`], in `systemctl
/// show`'s form.
pub const LOADED: &str = "DropInPaths,NeedDaemonReload,CPUQuotaPerSecUSec,ActiveState";

/// What [`UNIT_TEXT`]'s `CPUQuota=100%` reads back as in `systemctl show`.
pub const QUOTA_READ_BACK: &str = "1s";

/// After the gateway's unit is written or found as written, each round
/// (#7879): the unit systemd loaded is checked against its text. A failed
/// `daemon-reload` was only said once and never tried again, as the next
/// round found the same bytes; so a reload still owed
/// (`NeedDaemonReload=yes`) or a CPU cap not read back as
/// [`QUOTA_READ_BACK`] reloads systemd again and, with the gateway running,
/// `try-restart`s it so the cap applies, one line naming the check. A
/// drop-in is not lamp-update's: it is said and left as it is, with no
/// reload or restart, as neither would take it out. Any check still failing
/// is found again by the next round. No switch of its own: it only makes
/// systemd run lamp-update's own unit as written. Whether it asked for the
/// gateway's `try-restart`.
fn loaded_as_written(systemd: &mut dyn Systemd, say: &mut dyn FnMut(&str)) -> bool {
    let show = match systemd.gateway_unit_properties() {
        Ok(show) => show,
        Err(why) => {
            say(&format!(
                "{UNIT}: what systemd loaded is not checked: {why}"
            ));
            return false;
        }
    };
    let field = |name: &str| {
        show.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .map(str::trim)
    };
    if let Some(paths) = field("DropInPaths").filter(|paths| !paths.is_empty()) {
        say(&format!(
            "{UNIT} has a drop-in lamp-update did not write, left as it is: {}",
            crate::world::clean_line(paths)
        ));
        return false;
    }
    let failed = match (field("NeedDaemonReload"), field("CPUQuotaPerSecUSec")) {
        (Some("no"), Some(QUOTA_READ_BACK)) => return false,
        (Some("no"), quota) => format!(
            "its CPU cap reads back as {:?}, not {QUOTA_READ_BACK:?}",
            crate::world::clean_line(quota.unwrap_or("nothing"))
        ),
        (owed, _) => format!(
            "NeedDaemonReload is {:?}, not \"no\"",
            crate::world::clean_line(owed.unwrap_or("nothing"))
        ),
    };
    let running = matches!(
        field("ActiveState"),
        Some("active" | "activating" | "reloading")
    );
    if let Err(why) = systemd.reload() {
        say(&format!(
            "{UNIT} not loaded as written ({failed}), and systemd not reloaded: {why}"
        ));
        return false;
    }
    if !running {
        say(&format!(
            "{UNIT} not loaded as written ({failed}): systemd reloaded"
        ));
        return false;
    }
    match systemd.try_restart_gateway() {
        Ok(()) => say(&format!(
            "{UNIT} not loaded as written ({failed}): systemd reloaded and the gateway restarted"
        )),
        Err(why) => say(&format!(
            "{UNIT} not loaded as written ({failed}): systemd reloaded, the gateway not restarted: {why}"
        )),
    }
    true
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
    let started = ask(records, true, Shown::default())
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
    let socket = root.join(TASKS_APP_SOCKET.trim_start_matches('/'));
    install_said(root, records, run, now_unix_ms, say, kit, &mut || {
        said_default(&socket)
    })
}

/// [`install_with`], lamp-tasks' answer read through `said` (asked only
/// with [`Ask::by_default`], right before each start or restart of the
/// gateway).
fn install_said(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
    kit: &Kit,
    said: &mut dyn FnMut() -> Option<Said>,
) -> Result<Option<OpenclawInstall>, String> {
    let Ok(bytes) = fs::read(records.join(ASK)) else {
        return Ok(None);
    };
    let asked = serde_json::from_slice::<Ask>(&bytes).unwrap_or_default();
    let _lock = lock_named(records, LOCK, "OpenClaw")?;
    let by_default = asked.by_default;
    let mut wanted = || gateway_wanted(by_default, said());
    let said = if asked.remove {
        remove_entry_points(root, run, say);
        let _ = fs::remove_file(records.join(RECORD));
        None
    } else {
        let said = steps(
            root,
            records,
            run,
            now_unix_ms,
            say,
            kit,
            Shown {
                skills: asked.skills,
                tasks: asked.tasks,
                one_path: asked.one_path,
            },
            &mut wanted,
        );
        if said.install == "failed" {
            remove_entry_points(root, run, say);
        }
        Some(said)
    };
    let _ = fs::remove_file(records.join(ASK));
    Ok(said)
}

/// The gateway stopped, then the wrapper, the namespace script and
/// [`SKILL_START`] taken out, the wrapper first. OpenClaw's folder (its
/// state among it) and the unit file (the round's) stay.
fn remove_entry_points(root: &Path, run: &mut Runner, say: &mut dyn FnMut(&str)) {
    if fs::symlink_metadata(unit_path(root)).is_ok()
        && let Err(why) = run(&["systemctl", "stop", UNIT].map(str::to_owned))
    {
        say(&format!("{UNIT} not stopped: {why}"));
    }
    for path in [WRAPPER, NS, SKILL_START] {
        match fs::remove_file(on(root, path)) {
            Ok(()) => say(&format!("{path} taken out")),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => say(&format!("{path} not taken out: {error}")),
        }
    }
    agent_ns::remove_hook_shim(root, &hermes_install::NS_SCRIPTS, say);
    sync_socket_dir(root, &mut |_| Ok(String::new()), Shown::default(), say);
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
/// written, [`SKILL_START`] made as the one run path wants it
/// ([`sync_launcher`]), after a new install lamp-tasks started again if it
/// runs, the gateway (re)started (the round enables it), [`ASIDE`]
/// removed, and agent `main`'s exec allow-list edited through the running
/// gateway ([`exec_allowlist`]). OpenClaw there already: only the
/// launcher and the allow-list, when not done yet.
///
/// One agent gateway at a time: `wanted` ([`gateway_wanted`], read right
/// before) says whether the gateway may be (re)started. Wanted (always with
/// [`BY_DEFAULT_SWITCH`] off), as above. Not wanted (another engine the
/// lamp's default), it is neither started nor restarted: OpenClaw's files
/// are installed, its allow-list waits for a gateway that runs, and
/// [`crate::gateway_keeper`] stops one still running. Unknown (lamp-tasks
/// not answering, as right after lamp-tasks' own restart), only a running one is
/// restarted (`try-restart`), never one that is down.
#[allow(clippy::too_many_arguments)]
fn steps(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
    kit: &Kit,
    shown: Shown,
    wanted: &mut dyn FnMut() -> Gateway,
) -> OpenclawInstall {
    // What the record says of agent main's entry before this run: kept
    // when no edit runs (no gateway), so a switch-off's removes stay due.
    let had = record(records).is_some_and(|said| said.exec_allowlist);
    if present(root, shown) && kit.built() {
        say(&format!(
            "OpenClaw {VERSION} is already at {OPENCLAW}: nothing installed"
        ));
        sync_launcher(root, shown, say);
        sync_socket_dir(root, run, shown, say);
        gateway_block(root, run, say, wanted);
        let before = record(records).filter(|said| said.install == "installed");
        if let Some(said) = &before
            && said.exec_allowlist == shown.one_path
        {
            return said.clone();
        }
        // Never a failure of the install: an add not done is retried. No
        // gateway running, nothing was run: the record stays as it was, so
        // a switch-off's removes are retried until they run.
        let ran = exec_allowlist(root, run, shown, say);
        let mut said = before.unwrap_or_else(|| OpenclawInstall::installed(now_unix_ms()));
        said.exec_allowlist = ran.unwrap_or(had);
        return write(records, said);
    }
    // The gateway's unit is the round's to write (the one-shot may not):
    // an earlier build's unit waits for that round, nothing changed and
    // nothing taken out. A working OpenClaw stays as it is, its gateway
    // running (#7135, V2's HOLD: a binary ahead of its units took it out).
    if fs::read(unit_path(root)).is_ok_and(|bytes| bytes != unit_text_for(shown).as_bytes()) {
        say(&format!(
            "OpenClaw's install waits: {UNIT} is not this build's yet; the next round writes it, and OpenClaw stays as it is until then"
        ));
        return record(records)
            .filter(|said| said.install != "installing")
            .unwrap_or_else(|| {
                write(records, OpenclawInstall::new("absent", None, now_unix_ms()))
            });
    }
    // #8225: what the record goes back to when OpenClaw's config check
    // did not finish, so the next round asks again.
    let unchecked = record(records)
        .filter(|said| said.install != "installing")
        .unwrap_or_else(|| OpenclawInstall::new("absent", None, now_unix_ms()));
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
        let ns = ns_text(shown);
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
    // The package pinned already, its namespace script gone (a failed
    // install took it out, 52e6; #6405 wrote it only before npm): written
    // again, as `--version` and the gateway run inside it (#7135).
    if pinned_there
        && !holds(root, NS, &ns_text(shown))
        && let Err(why) = write_root_file(root, NS, &ns_text(shown), 0o755)
    {
        return fail("entry", why, say);
    }
    // #8225: a check a signal ended, or one past its bound, gave no
    // verdict: nothing is taken out and the record goes back, so the next
    // round tries again. Only OpenClaw's own refusal fails the install.
    match gateway_config(root, run, say) {
        Err(why) if why.contains(lamp_checked_replace::DID_NOT_FINISH) => {
            say(&format!("OpenClaw is not installed now: {why}"));
            return write(records, unchecked);
        }
        Err(why) => return fail("config", why, say),
        Ok(_) => {}
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
    if let Err(why) = write_entry_points(root, shown) {
        return fail("entry", why, say);
    }
    sync_launcher(root, shown, say);
    sync_socket_dir(root, run, shown, say);
    if !fs::read(unit_path(root)).is_ok_and(|bytes| bytes == unit_text_for(shown).as_bytes()) {
        return fail(
            "gateway",
            format!("{UNIT} is not this build's: a round writes it"),
            say,
        );
    }
    // lamp-tasks merges OpenClaw's models and skills at its start. Before
    // the gateway's (re)start: lamp-tasks' runtime folder is made again
    // with it, and the gateway's namespace shows START_SOCKET_DIR as it is
    // when the gateway starts.
    if !pinned_there
        && let Err(why) =
            run(&["systemctl", "try-restart", "lamp-tasks.service"].map(str::to_owned))
    {
        say(&format!("lamp-tasks.service not started again: {why}"));
    }
    // Always on (the owner on #6226): started now, or restarted so one
    // running an older OpenClaw runs the new one. The round enables it.
    // A gateway past its start limit refuses a restart: its failed state
    // is reset first (#8238), a no-op for any other.
    // Only while OpenClaw's gateway is wanted (one agent gateway at a time).
    match wanted() {
        Gateway::Wanted => {
            if let Err(why) = run(&["systemctl", "reset-failed", UNIT].map(str::to_owned)) {
                say(&format!("{UNIT} not reset-failed: {why}"));
            }
            settled(root, run, say, false);
            if let Err(why) = run(&["systemctl", "restart", UNIT].map(str::to_owned)) {
                return fail("gateway", format!("systemctl restart {UNIT}: {why}"), say);
            }
        }
        Gateway::Unknown => {
            settled(root, run, say, false);
            if let Err(why) = run(&["systemctl", "try-restart", "--", UNIT].map(str::to_owned)) {
                return fail(
                    "gateway",
                    format!("systemctl try-restart {UNIT}: {why}"),
                    say,
                );
            }
            say(&format!(
                "{UNIT} restarted only if it ran: lamp-tasks did not say the lamp's agent; lamp-update starts it once OpenClaw is"
            ));
        }
        Gateway::NotWanted => say(&format!(
            "{UNIT} not started: OpenClaw is not the lamp's agent; lamp-update starts it once it is"
        )),
    }
    remove_aside(root, say);
    // Through the gateway just (re)started; an add not done is retried by
    // the next round, never a failure of the install.
    let exec_allowlist = exec_allowlist(root, run, shown, say).unwrap_or(had);
    let shown = match (shown.skills, shown.tasks) {
        (true, true) => {
            "its own folder, its work folder, the skills, lamp-skills' socket and lamp-control's socket"
        }
        (true, false) => "its own folder, its work folder, the skills and lamp-skills' socket",
        (false, true) => "its own folder, its work folder, the skills and lamp-control's socket",
        (false, false) => "its own folder, its work folder and the skills",
    };
    say(&format!(
        "OpenClaw {} on Node {} installed for {USER} at {OPENCLAW}; Harness runs it through {WRAPPER}, its gateway {UNIT} (127.0.0.1:{PORT}) always on, in a namespace that shows it only {shown}",
        kit.version, kit.node_version
    ));
    write(
        records,
        OpenclawInstall {
            exec_allowlist,
            ..OpenclawInstall::installed(now_unix_ms())
        },
    )
}

/// The `openclaw approvals` edits [`exec_allowlist`] runs for `shown`,
/// each OpenClaw's own allow-list edit of the running gateway's exec
/// approvals (`--gateway`), never a whole document set. With the one run
/// path on, agent [`RUN_AGENT`] given [`SKILL_START`] exactly (no glob, no
/// argument pattern), then [`OLD_RUN_START`] taken out of [`RUN_AGENT`]'s
/// and [`OLD_FENCE_AGENT`]'s entries; off, [`RUN_AGENT`]'s [`SKILL_START`]
/// and [`OLD_RUN_START`] taken out.
pub fn exec_allowlist_args(shown: Shown) -> Vec<[&'static str; 7]> {
    let edit = |op, agent, path| {
        [
            "approvals",
            "allowlist",
            op,
            "--gateway",
            "--agent",
            agent,
            path,
        ]
    };
    if shown.one_path {
        vec![
            edit("add", RUN_AGENT, SKILL_START),
            edit("remove", RUN_AGENT, OLD_RUN_START),
            edit("remove", OLD_FENCE_AGENT, OLD_RUN_START),
        ]
    } else {
        vec![
            edit("remove", RUN_AGENT, SKILL_START),
            edit("remove", RUN_AGENT, OLD_RUN_START),
        ]
    }
}

/// [`exec_allowlist_args`] run as `lampos` through [`WRAPPER`], which joins
/// the running gateway's namespaces (the form that worked on a lamp; run
/// in a namespace of its own with no target, `approvals` exits 1 while the
/// gateway runs). Only with the gateway's unit active: else nothing is
/// run, said in one line, none returned, and the next round asks again.
/// A remove that fails is said and passed over; an add that fails is said
/// and not done. [`OLD_FENCE_AGENT`]'s remove fails on a lamp once that
/// agent is gone from OpenClaw's config ("Config unavailable; cannot
/// validate --agent"): expected, said in one line as harmless, and
/// nothing depends on it. Any other edit that fails (#8444: lamp-control
/// restarted during it, and `PartOf=` restarted the gateway under it)
/// waits, once per run, until the gateway is active, listens and is past
/// its first start ([`settled`]), and is run once more; failing again it
/// is said in its own line. Whether agent [`RUN_AGENT`]'s [`SKILL_START`]
/// entry was added: never with the one run path off. OpenClaw's config
/// and its `tools.exec` are never touched.
fn exec_allowlist(
    root: &Path,
    run: &mut Runner,
    shown: Shown,
    say: &mut dyn FnMut(&str),
) -> Option<bool> {
    if !gateway_active(run) {
        say(&format!(
            "OpenClaw's exec allow-list left as it is: {UNIT} is not running; the next round tries again"
        ));
        return None;
    }
    let mut added = false;
    let mut waited = false;
    for args in exec_allowlist_args(shown) {
        let mut ran = run(&allowlist_argv(args));
        // A gateway just (re)started is not ready for the add at first: its
        // namespace does not show Node yet, then it answers "Config
        // unavailable" until its config is read (#7135, c0a4: the first
        // run's exec was denied). The add is tried again, ADD_TRIES times
        // at most, ADD_AGAIN apart.
        for _ in 1..ADD_TRIES {
            match &ran {
                Err(why) if args[2] == "add" && gateway_not_ready(why) => {
                    std::thread::sleep(ADD_AGAIN);
                    ran = run(&allowlist_argv(args));
                }
                _ => break,
            }
        }
        // #8444: a gateway restarted under the edit; the old fence agent's
        // remove fails on every lamp and is never run again.
        if ran.is_err() && args[5] != OLD_FENCE_AGENT {
            if !waited {
                settled(root, run, say, true);
                waited = true;
            }
            ran = run(&allowlist_argv(args));
        }
        match ran {
            Ok(_) => added |= args[2] == "add",
            Err(why) if args[2] == "add" => say(&format!(
                "OpenClaw's exec allow-list still does not let agent {} run {} after the gateway settled and the add ran again: {why}; the next round tries again (#8444)",
                args[5], args[6]
            )),
            Err(_) if args[5] == OLD_FENCE_AGENT => say(&format!(
                "OpenClaw's exec allow-list: agent {OLD_FENCE_AGENT}'s old entry, if any, stays (harmless: the CLI cannot remove it once that agent is gone)"
            )),
            Err(why) => say(&format!(
                "OpenClaw's exec allow-list: {} {} still not taken out after the gateway settled and the remove ran again: {why} (#8444)",
                args[5], args[6]
            )),
        }
    }
    if added {
        say(&format!(
            "OpenClaw's exec allow-list lets agent {RUN_AGENT} run {SKILL_START} alone"
        ));
    } else if !shown.one_path {
        say(&format!(
            "OpenClaw's exec allow-list no longer lets agent {RUN_AGENT} run {SKILL_START}"
        ));
    }
    Some(added)
}

/// What OpenClaw's CLI says while a gateway just started has not read its
/// config yet (`approvals allowlist add --gateway`).
pub const CONFIG_NOT_READY: &str = "Config unavailable";
/// What the wrapper says when it joins a gateway just (re)started whose
/// namespace does not show Node yet (c0a4, 21:22:21: `/usr/bin/env:
/// 'node': No such file or directory`), before [`CONFIG_NOT_READY`]: one
/// constant with the checked replace's retry, so the two never drift.
pub use lamp_checked_replace::NODE_NOT_READY;

/// Whether an allow-list add was refused only because the gateway just
/// (re)started is not ready yet ([`NODE_NOT_READY`], [`CONFIG_NOT_READY`]).
fn gateway_not_ready(why: &str) -> bool {
    why.contains(NODE_NOT_READY) || why.contains(CONFIG_NOT_READY)
}

/// How many times an allow-list add is run while the gateway is not ready
/// ([`gateway_not_ready`]).
pub const ADD_TRIES: u32 = 40;
/// The wait between those adds: 40 × 3 s, two minutes at most (c0a4 was
/// not ready for about 40 s; a slower card takes longer).
pub const ADD_AGAIN: Duration = if cfg!(test) {
    Duration::from_millis(1)
} else {
    Duration::from_secs(3)
};

/// [`gateway_block`]'s waits for a gateway not there to check the block:
/// `lamp_checked_replace::GATEWAY_WAITS`, a millisecond each in tests, as
/// [`ADD_AGAIN`] is.
const GATEWAY_WAITS: [Duration; 8] = if cfg!(test) {
    [Duration::from_millis(1); 8]
} else {
    lamp_checked_replace::GATEWAY_WAITS
};

/// Whether the gateway's unit is active (`systemctl is-active`).
fn gateway_active(run: &mut Runner) -> bool {
    run(&["systemctl", "is-active", "--quiet", UNIT].map(str::to_owned)).is_ok()
}

/// How long after its start a listening gateway is past its startup drain
/// (#8324): kit-8306b on lamp-52e6 saw :18789 first listen 46 s after the
/// start and a stop at that first listen drain 79.9 s (OpenClaw logs a
/// startup budget of `drain=75000ms`); a stop 60 s or more after the
/// listen took under 10 s. 150 s from the start is that listen, those 60 s
/// and a margin.
pub const SETTLED_AFTER_START: Duration = Duration::from_secs(150);
/// How long a listen [`settled`] saw begin is waited past, at least: the
/// 60 s of kit-8306b's loaded stop and 30 s of margin. It also covers a
/// gateway whose start systemd can't say.
pub const SETTLED_AFTER_LISTEN: Duration = Duration::from_secs(90);
/// The most [`settled`] waits: past it the restart is done anyway.
pub const SETTLE_AT_MOST: Duration = Duration::from_secs(300);
/// How often [`settled`] looks. Its waits are counted in these steps, not
/// read off a clock (each look's own time only lengthens the wait).
pub const SETTLE_EVERY: Duration = Duration::from_secs(2);
/// The sleep of one step: [`SETTLE_EVERY`], shortened in tests.
const SETTLE_SLEEP: Duration = if cfg!(test) {
    Duration::from_millis(1)
} else {
    SETTLE_EVERY
};

/// #8324: before a restart of the gateway in Harness's one-shot (the
/// install's in `steps`, the gateway block's in [`gateway_block`]), a
/// running gateway still in its first start is waited for until it is past
/// its startup drain: it listens on [`PORT`], [`SETTLED_AFTER_START`] has
/// passed since systemd started its current process, and
/// [`SETTLED_AFTER_LISTEN`] since a listen seen to begin here (one look
/// not listening forgets the listen seen). [`SETTLE_AT_MOST`] at most,
/// looking every [`SETTLE_EVERY`]. A stop at the first listen drained
/// 79.9 s against the then 90 s `TimeoutStopSec` (kit-8306b, 150 s since #8320); a second stop
/// seconds after a start was SIGKILLed and left OpenClaw's state
/// unreadable (gate G on lamp-52e6). The earlier start may be
/// `gateway_config`'s supervisor restart, a round's `gateway_on`, a crash's
/// `Restart=always` during the wait, or a boot.
///
/// The start is re-read at each look (`ExecMainStartTimestampMonotonic`),
/// so a process started again during the wait is waited for anew, and
/// compared with `/proc/uptime` read once at entry plus the steps waited:
/// both count from the boot, so an NTP jump after a boot on a lamp with no
/// RTC moves neither (the lamp never suspends, where they would part). A
/// gateway not running, or a listen not readable, waits for nothing;
/// listening at the first look with its start or the uptime not read, it
/// is restarted at once.
///
/// With `retry` (#8444, [`exec_allowlist`]'s edit that failed), it waits
/// the same before that edit runs again, and a gateway not active (being
/// stopped or started by a `PartOf=` restart) counts as not listening, so
/// it is waited for too, [`SETTLE_AT_MOST`] at most.
fn settled(root: &Path, run: &mut Runner, say: &mut dyn FnMut(&str), retry: bool) {
    if !retry && !gateway_active(run) {
        return;
    }
    let (then, waits) = if retry {
        (
            "the allow-list edit run again",
            "the allow-list edit's retry",
        )
    } else {
        ("restarted", "the restart")
    };
    let uptime = fs::read_to_string(on(root, crate::reset_hold::UPTIME))
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(Duration::from_secs_f64);
    let sport = format!("sport = :{PORT}");
    let mut listen_seen: Option<Duration> = None;
    let mut quiet = false;
    let mut waited = Duration::ZERO;
    loop {
        let age = uptime
            .zip(gateway_start(run))
            .map(|(uptime, start)| (uptime + waited).saturating_sub(start));
        let listening = if retry && !gateway_active(run) {
            false
        } else {
            match run(&["ss", "-Hltn", &sport].map(str::to_owned)) {
                Ok(out) => !out.trim().is_empty(),
                Err(why) => {
                    say(&format!(
                        "{UNIT}: its listen on {PORT} not read: {why}; {then} now"
                    ));
                    return;
                }
            }
        };
        if !listening {
            listen_seen = None;
            quiet = true;
        } else if quiet && listen_seen.is_none() {
            listen_seen = Some(waited);
        }
        let past_start = age.is_none_or(|age| age >= SETTLED_AFTER_START);
        let past_listen = listen_seen.is_none_or(|at| waited - at >= SETTLED_AFTER_LISTEN);
        if listening && past_start && past_listen {
            if waited > Duration::ZERO {
                say(&format!(
                    "{UNIT} past its startup after {} s more: {then} (#8324)",
                    waited.as_secs()
                ));
            }
            return;
        }
        if waited >= SETTLE_AT_MOST {
            say(&format!(
                "{UNIT} still in its startup after {} s: {then} anyway (#8324)",
                waited.as_secs()
            ));
            return;
        }
        if waited == Duration::ZERO {
            say(&format!(
                "{UNIT} is in its startup (listening on {PORT}: {listening}): {waits} waits until it is past it (#8324)"
            ));
        }
        std::thread::sleep(SETTLE_SLEEP);
        waited += SETTLE_EVERY;
    }
}

/// When systemd started the gateway's current process, on the monotonic
/// clock (`ExecMainStartTimestampMonotonic`, microseconds since the boot);
/// none when it can't say or never started one.
fn gateway_start(run: &mut Runner) -> Option<Duration> {
    let said = run(&[
        "systemctl",
        "show",
        "--property=ExecMainStartTimestampMonotonic",
        "--value",
        "--",
        UNIT,
    ]
    .map(str::to_owned))
    .ok()?;
    let micros: u64 = said.trim().parse().ok()?;
    (micros > 0).then(|| Duration::from_micros(micros))
}

/// An [`exec_allowlist_args`] edit as it is run: as `lampos`, through
/// [`WRAPPER`], into the running gateway.
fn allowlist_argv(args: [&str; 7]) -> Vec<String> {
    let mut argv = vec![WRAPPER];
    argv.extend(args);
    as_lampos(&argv)
}

/// A round's check of agent [`RUN_AGENT`]'s [`SKILL_START`] entry with the
/// one run path on: with the gateway active, the same add the one-shot
/// runs (as `lampos`, through [`WRAPPER`]), which OpenClaw takes without
/// writing when the entry is there. None when the gateway is not active
/// (nothing run, nothing said: the round started it); else whether the
/// add was taken, a refusal said in one line. The old globs' removes are
/// the one-shot's alone. A gateway active but not ready yet (at a boot,
/// [`gateway_not_ready`]) is asked again as the one-shot's add is; still
/// not ready, it is None: unknown, not refused, so the entry is never
/// marked gone and OpenClaw never reinstalled for it (#7135, c0a4's
/// reboot at 22:59:00), and the next round checks again.
fn keep_run_entry(run: &mut Runner, say: &mut dyn FnMut(&str)) -> Option<bool> {
    add_run_entry(run, say, ADD_TRIES)
}

/// [`keep_run_entry`]'s add run once, never waited for: for
/// [`crate::gateway_keeper`]'s looks, which hold the gateways' lock and so
/// must never wait for a gateway to be ready (a stop of the other engine's
/// gateway would wait behind it). A gateway not active or not ready yet is
/// None, with nothing said: the keeper tries again at its next look.
pub fn run_entry_once(run: &mut Runner, say: &mut dyn FnMut(&str)) -> Option<bool> {
    add_run_entry(run, say, 1)
}

/// The add, `tries` times at most while the gateway is not ready, each
/// [`ADD_AGAIN`] apart; a last "not ready" is said only after a wait.
fn add_run_entry(run: &mut Runner, say: &mut dyn FnMut(&str), tries: u32) -> Option<bool> {
    if !gateway_active(run) {
        return None;
    }
    let add = exec_allowlist_args(Shown {
        one_path: true,
        ..Shown::default()
    })[0];
    let mut ran = run(&allowlist_argv(add));
    for _ in 1..tries {
        match &ran {
            Err(why) if gateway_not_ready(why) => {
                std::thread::sleep(ADD_AGAIN);
                ran = run(&allowlist_argv(add));
            }
            _ => break,
        }
    }
    match ran {
        Ok(_) => Some(true),
        Err(why) if tries <= 1 && gateway_not_ready(&why) => None,
        Err(why) if gateway_not_ready(&why) => {
            say(&format!(
                "OpenClaw's gateway is not ready yet, so agent {RUN_AGENT}'s allow-list entry is not known: {why}; the next round checks again"
            ));
            None
        }
        Err(why) => {
            say(&format!(
                "OpenClaw's exec allow-list does not let agent {RUN_AGENT} run {SKILL_START}: {why}; the one-shot is asked to add it"
            ));
            Some(false)
        }
    }
}

/// [`SKILL_START`] there as `shown` wants it: with the one run path on, a
/// regular file, 0755, holding [`TASKS_BINARY`]'s bytes; off, not there.
/// Nothing is due when lamp-tasks is not installed.
fn launcher_due(root: &Path, shown: Shown) -> bool {
    let place = on(root, SKILL_START);
    if !shown.one_path {
        return fs::symlink_metadata(&place).is_ok();
    }
    match fs::read(on(root, TASKS_BINARY)) {
        Ok(source) => !launcher_holds(&place, &source),
        Err(_) => false,
    }
}

fn launcher_holds(place: &Path, source: &[u8]) -> bool {
    fs::symlink_metadata(place).is_ok_and(|meta| meta.is_file() && meta.mode() & 0o7777 == 0o755)
        && fs::read(place).is_ok_and(|bytes| bytes == source)
}

/// [`START_SOCKET_DIR`] made at once with the one run path on (`lampos`'s,
/// 0755; the gateway's unit makes it at each start too), taken out off.
/// Never a failure of the install: what is not done is said.
fn sync_socket_dir(root: &Path, run: &mut Runner, shown: Shown, say: &mut dyn FnMut(&str)) {
    if shown.one_path {
        let made = [
            "install",
            "-d",
            "-o",
            USER,
            "-g",
            USER,
            "-m",
            "0755",
            START_SOCKET_DIR,
        ];
        if let Err(why) = run(&made.map(str::to_owned)) {
            say(&format!("{START_SOCKET_DIR} not made: {why}"));
        }
        return;
    }
    match fs::remove_dir_all(on(root, START_SOCKET_DIR)) {
        Ok(()) => say(&format!(
            "{START_SOCKET_DIR} taken out: the one run path is off"
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => say(&format!("{START_SOCKET_DIR} not taken out: {error}")),
    }
}

/// [`SKILL_START`] made what `shown` wants ([`launcher_due`]): with the
/// one run path on, a byte copy of [`TASKS_BINARY`] (never a link) written
/// whole beside and renamed in, root's, 0755, when it is missing or not
/// those bytes; off, taken out. Never a failure of the install: what is
/// not done is said, and the next round asks again.
fn sync_launcher(root: &Path, shown: Shown, say: &mut dyn FnMut(&str)) {
    let place = on(root, SKILL_START);
    if !shown.one_path {
        match fs::remove_file(&place) {
            Ok(()) => say(&format!("{SKILL_START} taken out: the one run path is off")),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => say(&format!("{SKILL_START} not taken out: {error}")),
        }
        return;
    }
    let source = match fs::read(on(root, TASKS_BINARY)) {
        Ok(source) => source,
        Err(error) => {
            say(&format!(
                "{SKILL_START} not written: no {TASKS_BINARY}: {error}"
            ));
            return;
        }
    };
    if launcher_holds(&place, &source) {
        return;
    }
    match write_whole(&place, &source, 0o755) {
        Ok(()) => say(&format!("{SKILL_START} written: a copy of {TASKS_BINARY}")),
        Err(error) => say(&format!("{SKILL_START} not written: {error}")),
    }
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
/// none, the block's own keys set in one there (every other key kept, in
/// the block and out of it) when it lacks them or holds others (#7868: a
/// round runs it too, [`gateway_block`]). Whether it wrote is returned. A
/// config lamp-update cannot read as a JSON object is left as it is, and
/// said. The new text is checked by
/// OpenClaw's own `config validate --json` ([`validate_argv`], #7868) on a
/// file beside the config, `lampos`'s, before it is renamed into place
/// (`lamp_checked_replace`, lamp-tasks' checked replace): refused, the
/// config stays byte for byte, nothing is left beside it, and the install
/// fails (`config`) with OpenClaw's words. A check that did not finish (a
/// signal, its bound: `lamp_checked_replace::check_did_not_finish`,
/// #8225) is no refusal: the config stays too, and the error says
/// [`NOT_CHECKED`].
fn gateway_config(
    root: &Path,
    run: &mut Runner,
    say: &mut dyn FnMut(&str),
) -> Result<bool, String> {
    let token_path = on(root, GATEWAY_TOKEN);
    let (token, made) = match kept_token(root) {
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
    let gateway = gateway_block_wanted(&token);
    let config_path = on(root, CONFIG);
    // Seventh's P2 on #7872: held from the read to the rename, as every
    // writer of the config does, so lamp-tasks' merge and this block never
    // drop each other's.
    let _held = lamp_checked_replace::hold(&on(root, STATE), lamp_checked_replace::LOCK_WITHIN)
        .map_err(|error| format!("{STATE}: {error}"))?;
    let mut config = match fs::read(&config_path) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(serde_json::Value::Object(config)) => config,
            _ => {
                say(&format!(
                    "{CONFIG} is not a JSON object lamp-update reads: left as it is"
                ));
                return Ok(false);
            }
        },
        Err(error) if error.kind() == ErrorKind::NotFound => serde_json::Map::new(),
        Err(error) => return Err(format!("{CONFIG}: {error}")),
    };
    let block = config.entry("gateway").or_insert(serde_json::Value::Null);
    if !set_within(block, gateway) && !made {
        return Ok(false);
    }
    let text = serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?;
    // #7868: checked by OpenClaw itself before it is renamed into place,
    // as lamp-tasks' writes are (#7108). With no namespace script OpenClaw
    // can't run, so there is nothing to check with: written unchecked.
    let ids = if fs::metadata(on(root, NS)).is_ok() {
        Some((install::uid_of(run, USER)?, gid_of(run, USER)?))
    } else {
        say(&format!(
            "{CONFIG} written unchecked: no OpenClaw namespace at {NS}"
        ));
        None
    };
    let mut check = |file: &Path| -> Result<(), String> {
        let Some((uid, gid)) = &ids else {
            return Ok(());
        };
        let file = Path::new("/").join(file.strip_prefix(root).unwrap_or(file));
        let file = file.to_string_lossy();
        give_to_lampos(run, &file)?;
        let joined = run(&MAIN_PID.map(str::to_owned)).is_ok_and(|said| has_main_pid(&said));
        run(&validate_argv(uid, gid, &file, joined))
            .map(drop)
            .map_err(|why| {
                let said = lamp_checked_replace::redact(&why);
                if lamp_checked_replace::check_did_not_finish(&why) {
                    format!(
                        "{} ({said}): {NOT_CHECKED}",
                        lamp_checked_replace::DID_NOT_FINISH
                    )
                } else {
                    format!("OpenClaw's own config check refused it: {said}")
                }
            })
    };
    lamp_checked_replace::replace_checked(
        &config_path,
        &text,
        rustix::fs::Mode::from_raw_mode(0o600),
        &mut check,
    )
    .map_err(|error| format!("{CONFIG}: {error}"))?;
    give_to_lampos(run, CONFIG).map(|()| true)
}

/// What a `gateway` block whose check did not finish says (#8225): on
/// lamp-52e6 the gateway the check joined restarted under it, "Killed".
const NOT_CHECKED: &str = "not checked now, kept as it was; the next round tries again";

/// `wanted`'s keys set in `have`, an object's keys within each one, every
/// other key of `have` kept; whether anything changed.
fn set_within(have: &mut serde_json::Value, wanted: serde_json::Value) -> bool {
    match (have, wanted) {
        (serde_json::Value::Object(have), serde_json::Value::Object(wanted)) => {
            let mut changed = false;
            for (key, value) in wanted {
                changed |= set_within(have.entry(key).or_insert(serde_json::Value::Null), value);
            }
            changed
        }
        (have, wanted) if *have != wanted => {
            *have = wanted;
            true
        }
        _ => false,
    }
}

/// The gateway's token kept in [`GATEWAY_TOKEN`] (a file, not empty).
fn kept_token(root: &Path) -> Option<String> {
    let token_path = on(root, GATEWAY_TOKEN);
    fs::symlink_metadata(&token_path)
        .ok()
        .filter(|meta| meta.is_file())
        .and_then(|_| fs::read_to_string(&token_path).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// The `gateway` block the install writes, with `token`.
fn gateway_block_wanted(token: &str) -> serde_json::Value {
    serde_json::json!({
        "mode": "local",
        "bind": "loopback",
        "port": PORT,
        "auth": {"mode": "token", "token": token},
    })
}

/// Whether [`gateway_config`] would write (#7868), read only: a round
/// checks this, as its own sandbox can't write OpenClaw's state folder
/// (`ProtectSystem=strict`, which no round changes), and a write due asks
/// Harness's one-shot ([`gateway_block`]). No token yet, or no config, is
/// due; a config not read as a JSON object is not (the write leaves it).
fn gateway_block_due(root: &Path) -> bool {
    let Some(token) = kept_token(root) else {
        return true;
    };
    let config = match fs::read(on(root, CONFIG)) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(serde_json::Value::Object(config)) => config,
            _ => return false,
        },
        Err(error) => return error.kind() == ErrorKind::NotFound,
    };
    let mut block = config.get("gateway").cloned().unwrap_or_default();
    set_within(&mut block, gateway_block_wanted(&token))
}

/// In Harness's one-shot over an OpenClaw there, asked by a round that
/// found the `gateway` block due ([`gateway_block_due`], #7868, as #7879
/// checks the gateway's unit each round): a reset whose cut was not taken
/// removes `openclaw.json`, and a lamp with OpenClaw installed runs no
/// install, so nothing else writes the block again. Missing or not as the
/// install writes it, it is written again through OpenClaw's own check
/// ([`gateway_config`]), and the gateway, when it runs, restarted on it
/// (`try-restart`, past its first start, [`settled`], #8324; the round's
/// [`gateway_on`] starts one that does not).
/// Refused, `openclaw.json` stays byte for byte and nothing is restarted.
/// One line, with no values of the config; still due, the next round asks
/// again. No switch of its own: it only keeps what the install writes.
///
/// Its check joins a running gateway ([`validate_argv`]), and one just
/// (re)started may not be there to join yet: only that refusal
/// (`lamp_checked_replace::gateway_not_there`) is tried again, after each
/// of [`GATEWAY_WAITS`] (lamp-tasks' schedule, #7268), each said in one
/// line; past the last, it is refused as any refusal is.
fn gateway_block(
    root: &Path,
    run: &mut Runner,
    say: &mut dyn FnMut(&str),
    wanted: &mut dyn FnMut() -> Gateway,
) {
    let mut waits = GATEWAY_WAITS.iter();
    let written = loop {
        match gateway_config(root, run, say) {
            Err(why) if lamp_checked_replace::gateway_not_there(&why) => {
                let Some(wait) = waits.next() else {
                    break Err(why);
                };
                say(&format!(
                    "{CONFIG}'s gateway block waits for OpenClaw's gateway to check it; tried again in {} s",
                    wait.as_secs()
                ));
                std::thread::sleep(*wait);
            }
            done => break done,
        }
    };
    match written {
        Ok(false) => {}
        // Not wanted (another engine the lamp's default): the block is
        // written, the gateway not restarted, so the one-shot never keeps
        // one up beside the other engine's ([`steps`]).
        Ok(true) if wanted() == Gateway::NotWanted => say(&format!(
            "{CONFIG}'s gateway block was not as lamp-update writes it: written again and checked by OpenClaw; the gateway not restarted: OpenClaw is not the lamp's agent"
        )),
        Ok(true) => {
            // #8324: as the install's restart, never in a first start.
            settled(root, run, say, false);
            match run(&["systemctl", "try-restart", "--", UNIT].map(str::to_owned)) {
                Ok(_) => say(&format!(
                    "{CONFIG}'s gateway block was not as lamp-update writes it: written again, checked by OpenClaw, and the gateway restarted if it ran"
                )),
                Err(why) => say(&format!(
                    "{CONFIG}'s gateway block was not as lamp-update writes it: written again, checked by OpenClaw, but the gateway not restarted: {}",
                    crate::world::clean_line(&why)
                )),
            }
        }
        Err(why) => say(&format!(
            "{CONFIG}'s gateway block is not as lamp-update writes it and not written again, the gateway not restarted: {why}"
        )),
    }
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
    let mut argv: Vec<String> = RUNUSER.map(str::to_owned).into();
    argv.push("env".to_owned());
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

/// What [`as_lampos`] runs a command as `lampos` with.
const RUNUSER: [&str; 4] = ["runuser", "-u", USER, "--"];

/// What OpenClaw's own config check sees for `${LAMP_HERMES_TOKEN}`
/// ([`validate_argv`]): a stand-in, as the check reads the file and calls
/// no provider, and the forwarder's own token never goes on a command line.
const CHECK_TOKEN: &str = "LAMP_HERMES_TOKEN=lamp-config-check";

/// What prints the `MainPID` of OpenClaw's gateway's unit: a number not
/// `0` while it runs ([`has_main_pid`]), as [`WRAPPER`] finds the gateway.
pub const MAIN_PID: [&str; 7] = ["systemctl", "show", "-p", "MainPID", "--value", "--", UNIT];

/// Whether [`MAIN_PID`]'s output names a running gateway.
pub fn has_main_pid(said: &str) -> bool {
    let pid = said.trim();
    !pid.is_empty() && pid != "0"
}

/// [`MAIN_PID`] as a part not run through lamp-update's runner runs it,
/// `systemctl` by its full path (lamp-setup's reset, lamp-tasks).
pub fn main_pid_argv() -> Vec<String> {
    std::iter::once("/usr/bin/systemctl")
        .chain(MAIN_PID[1..].iter().copied())
        .map(str::to_owned)
        .collect()
}

/// Whether `main_pid` ([`main_pid_argv`] on the lamp) prints a running
/// gateway's `MainPID` ([`has_main_pid`]); not run, or failed, it is not.
pub fn gateway_is_running(main_pid: &[String]) -> bool {
    let Some((program, args)) = main_pid.split_first() else {
        return false;
    };
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| {
            out.status.success() && has_main_pid(&String::from_utf8_lossy(&out.stdout))
        })
}

/// OpenClaw's own check of the config in `file` (a path on the lamp beside
/// `openclaw.json`, never the live one, readable by `lampos`): `config
/// validate --json` as `lampos`, pointed at `file` by
/// `OPENCLAW_CONFIG_PATH`. Status 0 takes it. `joined` (the gateway's unit
/// has a [`MAIN_PID`]): through [`WRAPPER`], which joins the running
/// gateway's namespaces, as OpenClaw 2026.11.2 refuses a check from any
/// other while its gateway runs (`GatewayLockNamespaceError`, #7268); the
/// wrapper passes the forwarder's token and fails closed with no gateway.
/// Else, with no gateway and so no lock to clash with, inside OpenClaw's
/// namespace of its own, as the install's `--version` runs
/// ([`in_namespace`]), with [`CHECK_TOKEN`]. The install's `gateway` block
/// (#7868) and lamp-setup's reset cut both run it; lamp-tasks, already
/// `lampos`, runs its own-namespace form ([`own_namespace_validate_argv`],
/// #8060).
pub fn validate_argv(uid: &str, gid: &str, file: &str, joined: bool) -> Vec<String> {
    let config = format!("OPENCLAW_CONFIG_PATH={file}");
    if joined {
        return as_lampos(&[config.as_str(), WRAPPER, "config", "validate", "--json"]);
    }
    let mut argv = in_namespace(uid, gid, false, &["config", "validate", "--json"]);
    let at = argv
        .iter()
        .position(|arg| arg == "/usr/bin/unshare")
        .unwrap_or(argv.len());
    argv.insert(at, CHECK_TOKEN.to_owned());
    argv.insert(at, config);
    argv
}

/// [`validate_argv`] in OpenClaw's namespace of its own (not `joined`),
/// for a caller that is `lampos` already (lamp-tasks' `openclaw.json`
/// writer, #8060): the same command, without `runuser`.
pub fn own_namespace_validate_argv(uid: &str, gid: &str, file: &str) -> Vec<String> {
    let mut argv = validate_argv(uid, gid, file, false);
    argv.drain(..RUNUSER.len());
    argv
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

/// The namespace script, the shared hook stand-in
/// ([`agent_ns::write_hook_shim`]), then the wrapper: root's, 0755, each
/// written whole beside and renamed in.
fn write_entry_points(root: &Path, shown: Shown) -> Result<(), String> {
    write_root_file(root, NS, &ns_text(shown), 0o755)?;
    agent_ns::write_hook_shim(root)?;
    write_root_file(root, WRAPPER, &wrapper_text_for(shown), 0o755)
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
