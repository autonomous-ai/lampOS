//! Hermes put on the lamp after Harness (#5978; docs/install.md, "Hermes on
//! the lamp"): on by default on a lamp installed as the product
//! ([`PRODUCT_DEFAULT`] is on: the product owner's decision, 10 Nov, which
//! the first builder allows with the schedule of
//! [`crate::hermes_schedule`]); `LAMP_HERMES_ON_LAMP=off` in lamp-update's
//! own unit turns it off, and any other lamp has it only with `on` there.
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
//! Hermes runs shell commands the model chooses, and as plain `lampos` they
//! could reach every lamp service that runs as `lampos`: lamp-cloud's relay
//! key, the eyes' sockets (the camera on) and face prints, the body's
//! motors, the person's tasks, and the camera, mic and serial ports through
//! `lampos`'s groups. So [`WRAPPER`] never starts Hermes but through
//! `unshare --map-root-user --mount --pid --kill-child --mount-proc` (its
//! own PID namespace and `/proc`, so no lamp process, lamp-cloud's
//! `/proc/<pid>/root` among them, is visible) and [`NS`], which shows
//! Hermes, by an allow-list, only [`ALLOWED`] (its own folder, its work
//! folder and the skills) over empty read-only tmpfs on each of
//! [`OVERLAID`] (`/run`, `/var/lib`, `/etc/lampOS`), gives it its own
//! [`PRIVATE`] (`/tmp`), binds `/dev/null` over the devices of [`DEVICES`],
//! and then drops back to the caller's own ids in a nested user namespace,
//! where those mounts are locked. Hermes is given only lamp-cloud's local
//! forwarder token ([`TOKEN`], as [`TOKEN_ENV`]), never the relay key.
//!
//! **The lamp's skills, behind their own switch** (#6128). With
//! [`SKILLS_SWITCH`] (`LAMP_HERMES_SKILLS` in lamp-update's own unit, on
//! by default on a product lamp, the product owner's decision: a person has
//! no way to turn it on; `off` there turns it off) the namespace script is written as [`ns_text`]`(true)`,
//! which also shows Hermes [`SKILLS_SOCKET`] (lamp-skills' `tools.json`
//! and `call.sock`), read-only, so `lamp-tasks skill call NAME` reaches the
//! lamp as the voice's model does: through lamp-skills, which applies its
//! cooldowns, pause and policy, takes no requested-audio context from any
//! peer but the voice, and refuses another peer's call while the lamp is
//! private. Off, it is [`NS_TEXT`], and nothing under `/run` is shown. The
//! switch is carried in the script's own text, written by root, never in
//! anything `lampos` can change; a round that finds the other text asks the
//! one-shot to write it again ([`present`]).
//!
//! Before anything is installed, the namespace is tried as `lampos`
//! ([`userns_argv`]); a lamp where it does not work gets nothing,
//! `no_userns`. No unit, no daemon,
//! nothing at boot: Harness starts `hermes` in a pane only when a task is
//! sent to it.
//!
//! **When.** After a `serve` round that reached the channel, with the
//! switch on and Hermes missing ([`present`]), and only in its window
//! ([`crate::hermes_schedule::window`]): the round writes [`ASK`].
//! With Harness there, it starts Harness's one-shot
//! ([`crate::harness_install::UNIT`]) itself; with Harness missing, the
//! one-shot Harness's own round starts installs Hermes right after
//! Harness. A Hermes failure never fails Harness's install or the round:
//! it is recorded in [`RECORD`], its entry points ([`WRAPPER`], [`NS`])
//! taken out, and it is tried again [`RETRY_AFTER`] later (one stopped by
//! `serve`'s watch, [`carry_out`], in the next open window). With the switch
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
    agent_ns, harness_install,
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
/// unset: on, the product owner's decision (10 Nov), under the lead's
/// schedule ([`crate::hermes_schedule`]: the first round reached but for
/// the setup walk, frozen while someone speaks, stopped when the privacy
/// reading is lost); `off`
/// turns it off. Any other lamp with it unset is off.
pub const PRODUCT_DEFAULT: bool = true;
/// The skills switch (#6128), in lamp-update's own unit: `on` exactly
/// shows Hermes [`SKILLS_SOCKET`] ([`ns_text`]); read as [`SWITCH`] is,
/// with [`SKILLS_PRODUCT_DEFAULT`].
pub const SKILLS_SWITCH: &str = "LAMP_HERMES_SKILLS";
/// Whether a lamp installed as the product shows Hermes the skills' socket
/// with [`SKILLS_SWITCH`] unset: on, the product owner's decision (11 Nov),
/// as a person has no way to turn it on; lamp-skills' caller check (no
/// requested-audio context or audio op from any peer but the voice, no
/// other peer's call while the lamp is private) is what makes that safe.
/// `off` turns it off; any other lamp with it unset is off.
pub const SKILLS_PRODUCT_DEFAULT: bool = true;
/// The skill tasks switch (#6306), in lamp-update's own unit: the same
/// name lamp-tasks reads (`lamp_tasks::skill_task::SWITCH`), so one name
/// turns a skill's tasks on. `on` exactly shows Hermes [`CONTROL_SOCKET`]
/// ([`ns_text`]); off unless set so, on any lamp: no product unit sets it.
pub const TASKS_SWITCH: &str = "LAMP_SKILL_TASKS";

/// What [`NS`] shows Hermes beyond [`ALLOWED`], each by its own switch:
/// lamp-skills' socket folder ([`SKILLS_SWITCH`]) and lamp-control's
/// control socket ([`TASKS_SWITCH`]). Both off is [`NS_TEXT`]. And
/// whether every agent runs by the one run path ([`Shown::one_path`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// [`SKILLS_SOCKET`], read-only.
    pub skills: bool,
    /// [`CONTROL_SOCKET`] alone, read-only.
    pub tasks: bool,
    /// The one run path (#6685; [`agent_ns::DEFAULT_AGENT_SWITCH`] on, read
    /// only with [`TASKS_SWITCH`] on): Hermes's wrapper [`WRAPPER`] is the
    /// one every agent has ([`agent_ns::wrapper::one_path_text`] with
    /// Hermes's data, [`agent`]) and runs [`agent_ns::wrapper::NS`], never
    /// [`NS`]; every other agent's wrapper is the same text with its own
    /// data. Off, every wrapper and namespace script is as before.
    pub one_path: bool,
}

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
/// The folder Harness starts Hermes in, and every folder Harness may make
/// beside it: Harness's project `~/harnesses/lamp-tasks` in its home
/// (`lamp_tasks::runtime::MACHINES_PROJECT` under
/// [`crate::install::HARNESS_HOME`]). lamp-tasks makes its `lamp`
/// subfolder (with `AGENTS.md`) and Harness starts Hermes there; the
/// project, not only `lamp`, is shown because Harness names the project
/// and may start a task in another folder of it. Nothing else of
/// lamp-tasks' state folder (its `store`, Harness's own files) is.
pub const WORK: &str = "/var/lib/lampos-tasks/harnesses/lamp-tasks";
/// The lamp's skills, which Hermes reads in place: shown read-only.
pub const SKILLS: &str = "/var/lib/lampOS/skills";
/// The skills the owner's own computer pushed (`lamp_skills::PUSHED_DIR`):
/// with [`TASKS_SWITCH`] on, shown read-only to a skill's run alone
/// ([`NET_ENV`] `run`), when it is there, so a pushed skill's task finds
/// its script (`lamp_tasks::skill_task::PUSHED`).
pub const PUSHED: &str = "/var/lib/lampOS/skills-dev";
/// lamp-skills' tools folder (`tools.json` and `call.sock`), the voice's
/// `LAMP_SKILLS_DIR`: with [`SKILLS_SWITCH`] on, the one folder under
/// `/run` [`ns_text`] shows Hermes, read-only (a unix socket still
/// connects on a read-only mount), when it is there. Off, it is hidden as
/// every other lamp service's folder is.
pub const SKILLS_SOCKET: &str = "/run/lampos-skills";
/// lamp-control's socket a skill's run reaches with its one-run token
/// (`lamp_tasks::skill_task::CONTROL_SOCKET`): with [`TASKS_SWITCH`] on,
/// the one file under `/run/lampos-control` [`ns_text`] shows Hermes,
/// read-only, when it is there. Never the folder, so lamp-control's
/// owner socket (`admin.sock`) beside it stays hidden.
pub const CONTROL_SOCKET: &str = "/run/lampos-control/control.sock";
/// The variable a skill's run is started with (`run` exactly, set by
/// lamp-tasks' `skill_task::HermesRunner`, `lamp_tasks::skill_task::NET_ENV`)
/// to have a network namespace of its own: with [`TASKS_SWITCH`] on,
/// [`wrapper_text`] then adds `--net` to its first `unshare`, and [`NS`]
/// shows [`MODEL_SOCKET`] and starts [`RELAY`] in it. Opting in only takes
/// access away. Any other value, or unset, is the network as before:
/// Hermes started by Harness's panes never sets it.
pub const NET_ENV: &str = "LAMP_NS_NET";
/// lamp-tasks' bridge to lamp-cloud's model forwarder
/// (`lamp_tasks::net_bridge::MODEL_SOCKET`, `lampos`'s, 0600 in its 0700
/// runtime folder): with [`NET_ENV`] `run`, the one file under
/// `/run/lampos-tasks` [`NS`] shows, read-only.
pub const MODEL_SOCKET: &str = "/run/lampos-tasks/model.sock";
/// What [`NS`] starts inside a skill run's network namespace, in the
/// background before Hermes, as the caller's own ids in a nested user
/// namespace of its own: lamp-tasks' `net-relay`, listening on
/// lamp-cloud's forwarder address (`lamp_cloud::hermes_llm::PORT`) and
/// piping each connection to [`MODEL_SOCKET`]. `/opt` is not hidden, and
/// this is lamp-tasks.service's own `ExecStart` program.
pub const RELAY: &str = "/opt/lampOS/tasks/current/lamp-tasks";
/// The address [`RELAY`] listens on: where Hermes's model calls go
/// (`http://127.0.0.1:7741/v1/deepseek`).
pub const RELAY_LISTEN: &str = "127.0.0.1:7741";
/// All [`NS`] shows Hermes of what it lays [`OVERLAID`] over: its own
/// folder, the work folder and the skills (read-only), plus the file
/// `/etc/resolv.conf` resolves to when that is under `/run` (read-only,
/// so names still resolve). An allow-list: a lamp service's runtime or
/// state folder is hidden unless named here, so a new service is hidden
/// with no change (`no_lampos_service_folder_is_visible_to_hermes`).
pub const ALLOWED: [&str; 3] = [PLACE, WORK, SKILLS];
/// What [`NS`] lays an empty read-only tmpfs over, with only [`ALLOWED`]
/// put back: every lamp service's sockets and runtime files, systemd's
/// credentials (lamp-cloud's relay key), every state folder (lamp-cloud's,
/// with [`TOKEN`], among them) and the lamp's own settings.
pub const OVERLAID: [&str; 3] = ["/run", "/var/lib", "/etc/lampOS"];
/// Shared on the lamp, so [`NS`] gives Hermes an empty writable tmpfs of
/// its own on each.
pub const PRIVATE: [&str; 3] = ["/tmp", "/var/tmp", "/dev/shm"];
/// Devices `lampos`'s groups open (`video`, `audio`, `dialout`, `i2c`):
/// under `--map-root-user` the groups cannot be dropped, so [`NS`] binds
/// `/dev/null` over each character device these match: cameras, the
/// microphone and speaker, the servo and other serial ports, I2C, SPI,
/// GPIO, HID and the display.
pub const DEVICES: [&str; 14] = [
    "/dev/video*",
    "/dev/media*",
    "/dev/v4l-subdev*",
    "/dev/snd/*",
    "/dev/ttyACM*",
    "/dev/ttyUSB*",
    "/dev/ttyS*",
    "/dev/ttyAMA*",
    "/dev/i2c-*",
    "/dev/spidev*",
    "/dev/gpiochip*",
    "/dev/hidraw*",
    "/dev/fb*",
    "/dev/dri/*",
];
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
/// lamp process is there. Nothing else of the caller's changes, but
/// `HERMES_YOLO_MODE` is unset: Hermes asks its approvals (#6684); with
/// [`TASKS_SWITCH`] on, [`RUN_LINES`] decide it instead.
pub const WRAPPER_TEXT: &str = r#"#!/bin/sh
# lampOS: Hermes on the lamp (#5978; lampOS/docs/install.md, "Hermes on the lamp"). Written by lamp-update.
# Hermes runs as its caller (lampos, Harness's user), in a user, mount and PID namespace with its own /proc, where it
# sees only its own folder, its work folder and the skills, no lamp service's files or sockets, no camera, mic or
# serial device and no lamp process; it is given only lamp-cloud's local forwarder token. It is never run any other way.
T=$(cat /var/lib/lampos-cloud/hermes-token 2>/dev/null) && [ -n "$T" ] || {
  echo "hermes: no forwarder token at /var/lib/lampos-cloud/hermes-token; lamp-cloud makes it with LAMP_HERMES_ON_LAMP=on" >&2
  exit 1
}
LAMP_NS_UID=$(id -u) && LAMP_NS_GID=$(id -g) || exit 1
export LAMP_HERMES_TOKEN="$T" HERMES_HOME=/var/lib/lampos-hermes/.hermes LAMP_NS_UID LAMP_NS_GID
unset T HERMES_YOLO_MODE
exec /usr/bin/unshare --map-root-user --mount --pid --kill-child --mount-proc --propagation private /usr/local/lib/lampos/hermes-ns "$@"
"#;

/// The folder a skill's run starts in, as `<RUN_FOLDERS>/skill-<run-id>`
/// (#6510 Part 1 step 2): lamp-tasks' Harness project [`WORK`], where
/// lamp-tasks makes the run's folder (0700) and has Harness start the
/// engine (`lamp_tasks::skill_task::HarnessRunner`).
pub const RUN_FOLDERS: &str = WORK;
/// Where lamp-tasks leaves a run's one-run token, as `<run-id>`
/// (`lamp_tasks::run_token_file::RUNS_DIR`: tmpfs, 0600 in `lampos`'s
/// 0700 folder; the wrapper runs as `lampos` too).
pub const RUNS: &str = "/run/lampos-tasks/runs";
/// The lampos SDK a skill's script imports (`crate::sdk_tree::DIR`,
/// `lamp_tasks::skill_task::SDK_PYTHONPATH`): a run's `PYTHONPATH`.
pub const SDK_PYTHONPATH: &str = "/usr/local/lib/lampos/sdk-python";

/// The file Hermes's terminal sources before its session's login snapshot
/// (`terminal.shell_init_files`, written there by lamp-tasks'
/// `hermes_config::RUN_ENV`, #6684): Hermes's own launcher replaces the
/// `PYTHONPATH` it is started with, so a skill run's [`SDK_PYTHONPATH`]
/// never reaches its commands any other way. Root's, 0644: Hermes reads it
/// and cannot change it.
pub const RUN_ENV: &str = "/usr/local/lib/lampos/hermes-run-env.sh";

/// [`RUN_ENV`]'s text: [`SDK_PYTHONPATH`] put first on `PYTHONPATH` only
/// when `LAMP_CONTROL_SOCKET` is set, which only a skill run's start sets
/// (the wrapper's run lines); a person's own Hermes, or a spoken task,
/// gets nothing. Already first, it is not added again.
pub const RUN_ENV_TEXT: &str = "\
# lampOS: a skill run's lampos SDK on PYTHONPATH (lamp-update writes this file).
case ${LAMP_CONTROL_SOCKET-} in ?*)
  case ${PYTHONPATH-} in /usr/local/lib/lampos/sdk-python|/usr/local/lib/lampos/sdk-python:*) ;;
  *) PYTHONPATH=/usr/local/lib/lampos/sdk-python${PYTHONPATH:+:$PYTHONPATH}; export PYTHONPATH ;;
  esac ;;
esac
";

/// The lines a wrapper finds a skill's run by (#6510 Part 1 step 2), as
/// text for `concat!`: Harness passes an engine no variable of
/// lamp-tasks', so the run is named by its folder. Only when `pwd -P` is
/// exactly [`RUN_FOLDERS`]`/skill-<run-id>` with a plain run id (`a-z`,
/// `0-9`, `-`, at most 64 bytes) is it a run: then [`RUNS`]`/<run-id>`
/// is read once if it is a regular file (never through a link) and
/// deleted whatever it is, and no token there ends it before anything
/// runs (`no token for run <run-id>`), so a second start in the same
/// folder (a Harness relaunch, `resume`) gets none. A run's environment
/// then has the token ([`agent_ns::wrapper::TOKEN_ENV`]),
/// [`CONTROL_SOCKET`], [`SDK_PYTHONPATH`] and [`NET_ENV`] `run` (its own
/// network namespace), as lamp-tasks' `HermesRunner` gives them; the
/// token is assigned and exported, never a command's argument. Any other
/// folder: nothing here changes. These are the lines that find the run
/// ([`run_lines_read`] reads its token); [`run_lines`] and
/// [`one_run_lines`] are the whole block, which Hermes's own wrapper
/// ([`WRAPPER_TASKS_EDITS`]) and every agent's
/// ([`agent_ns::wrapper::wrapper_text`], [`agent_ns::wrapper::one_path_text`])
/// carry.
macro_rules! run_lines_find {
    () => {
        r#"# A skill's run (#6510): started by lamp-tasks through Harness in /var/lib/lampos-tasks/harnesses/lamp-tasks/skill-<run-id>,
# it gets its one-run token from /run/lampos-tasks/runs/<run-id>, read once and deleted, in its environment only.
P=$(pwd -P 2>/dev/null) || P=
R=
case $P in /var/lib/lampos-tasks/harnesses/lamp-tasks/skill-?*) R=${P#/var/lib/lampos-tasks/harnesses/lamp-tasks/skill-} ;; esac
case $R in *[!abcdefghijklmnopqrstuvwxyz0123456789-]*) R= ;; esac
[ ${#R} -le 64 ] || R=
"#
    };
}
pub(crate) use run_lines_find;
/// The lines that read a run's token file and give the run its
/// environment, after [`run_lines_find`], open: [`run_lines`] and
/// [`one_run_lines`] close them.
macro_rules! run_lines_read {
    () => {
        r#"if [ -n "$R" ]; then
  F=/run/lampos-tasks/runs/$R
  C=
  if [ -f "$F" ] && [ ! -L "$F" ]; then C=$(cat -- "$F") || C=; fi
  rm -f -- "$F" || { echo "${0##*/}: the token file of run $R is not deleted" >&2; exit 1; }
  [ -n "$C" ] || { echo "${0##*/}: no token for run $R" >&2; exit 1; }
  LAMP_CONTROL_TOKEN=$C LAMP_CONTROL_SOCKET=/run/lampos-control/control.sock PYTHONPATH=/usr/local/lib/lampos/sdk-python LAMP_NS_NET=run
  unset C
  export LAMP_CONTROL_TOKEN LAMP_CONTROL_SOCKET PYTHONPATH LAMP_NS_NET
"#
    };
}
pub(crate) use run_lines_read;
/// Hermes's own wrapper's run lines (with [`TASKS_SWITCH`] on and
/// `LAMP_SKILL_DEFAULT_AGENT` off, [`WRAPPER_TASKS_EDITS`], and the older
/// agent-namespace wrapper's): the one run path's lines
/// ([`one_run_lines`]) with Hermes's approvals decided in them, as before
/// the one run path (#6684).
macro_rules! run_lines {
    () => {
        concat!(
            $crate::hermes_install::run_lines_find!(),
            "unset HERMES_YOLO_MODE\n",
            $crate::hermes_install::run_lines_read!(),
            "  # Nobody answers a skill's run: Hermes asks no approval in it, its hardline list still holds (#6684).\n",
            "  HERMES_YOLO_MODE=1\n",
            "  export HERMES_YOLO_MODE\n",
            "fi\n"
        )
    };
}
pub(crate) use run_lines;
/// The one run path's run lines (#6685, with `LAMP_SKILL_DEFAULT_AGENT`
/// on): the same for every agent, naming none. What an agent needs only in
/// a skill's run is its own data ([`crate::agent_ns::wrapper::Agent::run_env`]).
macro_rules! one_run_lines {
    () => {
        concat!(
            $crate::hermes_install::run_lines_find!(),
            $crate::hermes_install::run_lines_read!(),
            "fi\n"
        )
    };
}
pub(crate) use one_run_lines;
/// [`run_lines`]'s text.
pub const RUN_LINES: &str = run_lines!();
/// [`one_run_lines`]'s text.
pub const ONE_RUN_LINES: &str = one_run_lines!();

/// What [`wrapper_text`] changes in [`WRAPPER_TEXT`] with [`TASKS_SWITCH`]
/// on, each found exactly once (a test holds it): [`RUN_LINES`] first, so
/// a skill's run started by Harness in its own folder has its token file
/// read and deleted before anything else; then with [`NET_ENV`] `run`
/// exactly (set by [`RUN_LINES`], or by lamp-tasks' `HermesRunner`),
/// `--net` is added to the first `unshare`, so the new network
/// namespace is owned by the new user namespace (one `unshare(2)` with
/// `CLONE_NEWUSER`), and the variable passed on to [`NS`]; any other
/// value is unset, so [`NS`] never sees `run` without `--net`.
/// Where a voice make's own home may be (`lamp_tasks::skill_make`, #6643,
/// the first builder's 19:14 ruling): a fresh, empty folder in the make's
/// folder in lamp-tasks' work folder, removed after. With [`TASKS_SWITCH`]
/// on, [`wrapper_text`] takes [`HOME_ENV`] only when it names one of these
/// (never with `..`), and [`ns_text`] then makes it Hermes's `HOME`, with
/// `HERMES_HOME` under it. For a make it also hides Hermes's lasting home
/// ([`HERMES_HOME`]) under an empty read-only tmpfs, with only the
/// checkout ([`CHECKOUT`]) shown again in it, read-only, so the program
/// still runs but a make never reads or leaves memory, sessions or state
/// there (#6643's HOLDs, 12 Nov 19:58 and 20:0x); leaves [`PLACE`] and
/// [`WORK`] read-only; and hides the rest of `WORK/lamp` under another,
/// showing only the make's own folder there, read-write. Any other value
/// is unset: Hermes started any other way is as before.
pub const MAKE_HOME_GLOB: &str = "/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home";
/// The variable a voice make's run names its own home in ([`MAKE_HOME_GLOB`]).
pub const HOME_ENV: &str = "LAMP_NS_HOME";
/// The line [`wrapper_text`] adds for [`HOME_ENV`], after the token is
/// exported: lamp-tasks checks it is there before it starts a make.
pub const WRAPPER_HOME_LINE: &str = "case ${LAMP_NS_HOME-} in *..*) unset LAMP_NS_HOME ;; /var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home) export HERMES_HOME=\"$LAMP_NS_HOME/.hermes\" LAMP_NS_HOME ;; *) unset LAMP_NS_HOME ;; esac\n";
/// The lines [`ns_text`] adds for [`HOME_ENV`], before the ids are
/// dropped (see [`MAKE_HOME_GLOB`]); any step failing ends it before
/// Hermes runs. lamp-tasks checks they are there before it starts a make.
pub const NS_HOME_LINE: &str = "if [ -n \"${LAMP_NS_HOME-}\" ]; then\n  F=${LAMP_NS_HOME%/home} X=/tmp/.lampos-mk\n  [ -d \"$LAMP_NS_HOME\" ] && [ -n \"$W\" ] && [ \"${F%/*}\" = \"$W/lamp\" ] || fail \"no $LAMP_NS_HOME\"\n  mkdir -m 0700 \"$X\" \"$X/f\" \"$X/c\" || fail \"no make staging\"\n  mount --bind \"$F\" \"$X/f\" || fail \"no $F\"\n  mount --bind \"$H/.hermes/hermes-agent\" \"$X/c\" || fail \"no Hermes checkout\"\n  mount -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$H/.hermes\" || fail \"$H/.hermes not hidden\"\n  mkdir \"$H/.hermes/hermes-agent\" && mount --move \"$X/c\" \"$H/.hermes/hermes-agent\" && ro \"$H/.hermes/hermes-agent\" || fail \"Hermes checkout not shown\"\n  ro \"$H/.hermes\"; ro \"$H\"; ro \"$W\"\n  mount -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$W/lamp\" || fail \"$W/lamp not hidden\"\n  mkdir \"$F\" && mount --move \"$X/f\" \"$F\" || fail \"$F not shown\"\n  ro \"$W/lamp\"\n  rmdir \"$X/f\" \"$X/c\" \"$X\" || fail \"make staging left\"\n  cd \"$F\" || fail \"$F not visible\"\n  HOME=$LAMP_NS_HOME\nfi\nunset LAMP_NS_HOME F X\n";

const WRAPPER_TASKS_EDITS: [(&str, &str); 3] = [
    (
        "# serial device and no lamp process; it is given only lamp-cloud's local forwarder token. It is never run any other way.\n",
        concat!(
            "# serial device and no lamp process; it is given only lamp-cloud's local forwarder token. It is never run any other way.\n\
             # LAMP_SKILL_TASKS=on (#6306): a skill's run, started with LAMP_NS_NET=run, also gets a network namespace of its own,\n\
             # with only lo: out only to control.sock and lamp-cloud's model forwarder, through lamp-tasks' bridge socket.\n",
            run_lines!()
        ),
    ),
    (
        "unset T HERMES_YOLO_MODE\nexec /usr/bin/unshare --map-root-user --mount ",
        "unset T\n\
         if [ \"${LAMP_NS_NET-}\" = run ]; then N=--net; else N=; unset LAMP_NS_NET; fi\n\
         exec /usr/bin/unshare --map-root-user $N --mount ",
    ),
    (
        "LAMP_NS_UID LAMP_NS_GID\nunset T\n",
        concat!(
            "LAMP_NS_UID LAMP_NS_GID\n",
            "case ${LAMP_NS_HOME-} in *..*) unset LAMP_NS_HOME ;; /var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home) export HERMES_HOME=\"$LAMP_NS_HOME/.hermes\" LAMP_NS_HOME ;; *) unset LAMP_NS_HOME ;; esac\n",
            "unset T\n"
        ),
    ),
];

/// [`WRAPPER`]'s text: [`WRAPPER_TEXT`], with [`WRAPPER_TASKS_EDITS`] when
/// `tasks` ([`TASKS_SWITCH`]), and the shared namespace piece's journal
/// line ([`agent_ns::with_hook_note`], #6530). Written by root beside
/// [`ns_text`]'s, with the same switch.
pub fn wrapper_text(tasks: bool) -> String {
    let mut text = WRAPPER_TEXT.to_owned();
    if tasks {
        for (from, to) in WRAPPER_TASKS_EDITS {
            text = text.replacen(from, to, 1);
        }
    }
    agent_ns::with_hook_note(&text, "hermes")
}

/// Hermes's own lines in the one run path's wrapper, at every start (#6685,
/// [`Shown::one_path`]): its forwarder token read from [`TOKEN`] (none ends
/// it, as in [`WRAPPER_TEXT`]) and given as [`TOKEN_ENV`] with its
/// [`HERMES_HOME`], and `HERMES_YOLO_MODE` unset, so Hermes asks its
/// approvals. Data of Hermes's: only in its wrapper's data block.
pub const AGENT_ENV: &str = r#"  T=$(cat /var/lib/lampos-cloud/hermes-token 2>/dev/null) && [ -n "$T" ] || fail "no forwarder token at /var/lib/lampos-cloud/hermes-token; lamp-cloud makes it with LAMP_HERMES_ON_LAMP=on"
  LAMP_HERMES_TOKEN=$T HERMES_HOME=/var/lib/lampos-hermes/.hermes
  export LAMP_HERMES_TOKEN HERMES_HOME
  unset T HERMES_YOLO_MODE
"#;
/// Hermes's own lines in a skill's run only (#6684): nobody answers a
/// skill's run, so Hermes asks no approval in it; its hardline list still
/// holds.
pub const AGENT_RUN_ENV: &str = "  # Nobody answers a skill's run: Hermes asks no approval in it, its hardline list still holds (#6684).
  HERMES_YOLO_MODE=1
  export HERMES_YOLO_MODE
";

/// Hermes's data in the one run path's wrapper ([`Shown::one_path`]): its
/// home [`PLACE`], shown at that path (its checkout's scripts and its
/// Python name it), its program [`HERMES`], [`AGENT_ENV`] and
/// [`AGENT_RUN_ENV`].
pub fn agent() -> agent_ns::wrapper::Agent {
    agent_ns::wrapper::Agent {
        engine: "hermes".to_owned(),
        home: PLACE.to_owned(),
        program: Some(HERMES.to_owned()),
        env: AGENT_ENV.to_owned(),
        run_env: AGENT_RUN_ENV.to_owned(),
    }
}

/// [`WRAPPER`]'s text for `shown`: with [`Shown::one_path`], the one run
/// path's wrapper with Hermes's data ([`agent`]); else [`wrapper_text`].
pub fn wrapper_for(shown: Shown) -> String {
    if shown.one_path {
        agent_ns::wrapper::one_path_text(&agent()).unwrap_or_default()
    } else {
        wrapper_text(shown.tasks)
    }
}

/// The namespace script [`WRAPPER`] runs for `shown`, and its text: with
/// [`Shown::one_path`], the one every agent runs
/// ([`agent_ns::wrapper::NS`], [`agent_ns::wrapper::ns_text`]); else
/// [`NS`] ([`ns_text`]).
pub fn ns_for(shown: Shown) -> (&'static str, String) {
    if shown.one_path {
        (agent_ns::wrapper::NS, agent_ns::wrapper::ns_text(shown))
    } else {
        (NS, ns_text(shown.skills, shown.tasks))
    }
}

/// [`NS`]: only inside a user namespace (refused in the lamp's own, where
/// it would hide the lamp's files from the whole lamp). An allow-list:
/// 1. a private tmpfs on each of [`PRIVATE`]; on the new `/tmp`, what
///    stays visible is staged: [`ALLOWED`] bound there (the work folder
///    and the skills only when there) and the file `/etc/resolv.conf`
///    resolves to when it is under `/run`;
/// 2. an empty tmpfs over each of [`OVERLAID`] (`/etc/lampOS` when there),
///    the mount points made in it and the staged mounts moved back; the
///    skills and `resolv.conf` remounted read-only (with the flags they
///    had, as a user namespace may only add flags), then each overlay;
/// 3. `/dev/null` bound over each character device of [`DEVICES`];
/// 4. back into the folder it was started in, now through the new mounts
///    (the old one would reach the hidden folders by `..`): one not
///    visible ends it;
/// 5. [`HERMES`] run with `HOME` its own folder, as the caller's own ids,
///    in a nested user namespace, where none of these mounts can be undone.
///
/// Any step failing ends it before Hermes runs. This is the text with
/// [`SKILLS_SWITCH`] off; [`ns_text`] gives the one with it on.
pub const NS_TEXT: &str = r#"#!/bin/sh
# lampOS: Hermes on the lamp, inside the namespace /usr/local/bin/hermes makes (#5978; lampOS/docs/install.md,
# "Hermes on the lamp"). Written by lamp-update. Root here only: it shows Hermes only its own folder, its work folder,
# the skills (read-only) and resolv.conf over empty read-only /run, /var/lib and /etc/lampOS, gives it its own /tmp,
# hides the camera, mic and serial devices, then drops back to the caller's own ids in a nested user namespace,
# where these mounts are locked. Any step failing ends it before Hermes runs.
read -r _ _ count < /proc/self/uid_map || exit 1
[ "$count" != 4294967295 ] || { echo "hermes-ns: only inside the namespace /usr/local/bin/hermes makes" >&2; exit 1; }
[ -n "${LAMP_NS_UID-}" ] && [ -n "${LAMP_NS_GID-}" ] || exit 1
H=/var/lib/lampos-hermes W=/var/lib/lampos-tasks/harnesses/lamp-tasks K=/var/lib/lampOS/skills S=/tmp/.lampos-ns
D=$(pwd -P) || exit 1
R=$(readlink -f /etc/resolv.conf) || R=
case $R in /run/?*) [ -f "$R" ] || R= ;; *) R= ;; esac
fail() { echo "hermes-ns: $1" >&2; exit 1; }
ro() { O=$(findmnt -no VFS-OPTIONS -M "$1") || fail "no mount at $1"; O=${O#r[ow]}; mount -o "remount,bind,ro$O" "$1" || fail "$1 not read-only"; }
mount -t tmpfs -o size=256m,mode=1777,nosuid,nodev none /tmp || fail "no /tmp"
mount -t tmpfs -o size=64m,mode=1777,nosuid,nodev none /var/tmp || fail "no /var/tmp"
if [ -d /dev/shm ]; then mount -t tmpfs -o size=64m,mode=1777,nosuid,nodev none /dev/shm || fail "no /dev/shm"; fi
mkdir -m 0700 "$S" "$S/h" "$S/w" "$S/k" || fail "no staging"
mount --bind "$H" "$S/h" || fail "no $H"
if [ -d "$W" ]; then mount --bind "$W" "$S/w" || fail "no $W"; else W=; fi
if [ -d "$K" ]; then mount --bind "$K" "$S/k" || fail "no $K"; else K=; fi
if [ -n "$R" ]; then touch "$S/r" && mount --bind "$R" "$S/r" || fail "no $R"; fi
mount -t tmpfs -o size=1m,mode=0755,nosuid,nodev,noexec none /run || fail "/run not hidden"
mount -t tmpfs -o size=1m,mode=0755,nosuid,nodev,noexec none /var/lib || fail "/var/lib not hidden"
if [ -d /etc/lampOS ]; then mount -t tmpfs -o size=4k,mode=0555,nosuid,nodev,noexec none /etc/lampOS || fail "/etc/lampOS not hidden"; fi
mkdir -p "$H" && mount --move "$S/h" "$H" || fail "$H not shown"
if [ -n "$W" ]; then mkdir -p "$W" && mount --move "$S/w" "$W" || fail "$W not shown"; fi
if [ -n "$K" ]; then mkdir -p "$K" && mount --move "$S/k" "$K" && ro "$K" || fail "$K not shown"; fi
if [ -n "$R" ]; then mkdir -p "${R%/*}" && touch "$R" && mount --move "$S/r" "$R" && ro "$R" || fail "$R not shown"; fi
rm -f "$S/r" && rmdir "$S/h" "$S/w" "$S/k" "$S" || fail "staging left"
mount -o remount,bind,ro,nosuid,nodev,noexec /run || fail "/run not read-only"
mount -o remount,bind,ro,nosuid,nodev,noexec /var/lib || fail "/var/lib not read-only"
if [ -d /etc/lampOS ]; then mount -o remount,bind,ro,nosuid,nodev,noexec /etc/lampOS || fail "/etc/lampOS not read-only"; fi
for d in /dev/video* /dev/media* /dev/v4l-subdev* /dev/snd/* /dev/ttyACM* /dev/ttyUSB* /dev/ttyS* /dev/ttyAMA* /dev/i2c-* /dev/spidev* /dev/gpiochip* /dev/hidraw* /dev/fb* /dev/dri/*; do
  [ -c "$d" ] || continue
  mount --bind /dev/null "$d" || fail "$d not hidden"
done
cd "$D" 2>/dev/null || { echo "hermes-ns: $D is not visible to Hermes; it starts in $H" >&2; cd "$H" 2>/dev/null; } || fail "neither $D nor $H is visible to Hermes"
U=$LAMP_NS_UID G=$LAMP_NS_GID HOME=$H
unset LAMP_NS_UID LAMP_NS_GID
export HOME
exec /usr/bin/unshare --user --map-user="$U" --map-group="$G" /var/lib/lampos-hermes/.local/bin/hermes "$@"
"#;

/// What [`ns_text`] changes in [`NS_TEXT`] with [`SKILLS_SWITCH`] on, each
/// found exactly once (a test holds it): [`SKILLS_SOCKET`] staged with the
/// rest before `/run` is hidden (skipped when the folder is not there),
/// moved back and remounted read-only, and its staging folder removed.
const SKILLS_EDITS: [(&str, &str); 6] = [
    (
        "# where these mounts are locked. Any step failing ends it before Hermes runs.\n",
        "# where these mounts are locked. Any step failing ends it before Hermes runs.\n\
         # LAMP_HERMES_SKILLS=on (#6128): it also shows lamp-skills' socket folder /run/lampos-skills, read-only, so Hermes\n\
         # reaches the lamp only through lamp-skills, as the voice does.\n",
    ),
    (
        "K=/var/lib/lampOS/skills S=/tmp/.lampos-ns\n",
        "K=/var/lib/lampOS/skills C=/run/lampos-skills S=/tmp/.lampos-ns\n",
    ),
    (
        "mkdir -m 0700 \"$S\" \"$S/h\" \"$S/w\" \"$S/k\" || fail \"no staging\"\n",
        "mkdir -m 0700 \"$S\" \"$S/h\" \"$S/w\" \"$S/k\" \"$S/c\" || fail \"no staging\"\n",
    ),
    (
        "if [ -d \"$K\" ]; then mount --bind \"$K\" \"$S/k\" || fail \"no $K\"; else K=; fi\n",
        "if [ -d \"$K\" ]; then mount --bind \"$K\" \"$S/k\" || fail \"no $K\"; else K=; fi\n\
         if [ -d \"$C\" ]; then mount --bind \"$C\" \"$S/c\" || fail \"no $C\"; else C=; fi\n",
    ),
    (
        "if [ -n \"$K\" ]; then mkdir -p \"$K\" && mount --move \"$S/k\" \"$K\" && ro \"$K\" || fail \"$K not shown\"; fi\n",
        "if [ -n \"$K\" ]; then mkdir -p \"$K\" && mount --move \"$S/k\" \"$K\" && ro \"$K\" || fail \"$K not shown\"; fi\n\
         if [ -n \"$C\" ]; then mkdir -p \"$C\" && mount --move \"$S/c\" \"$C\" && ro \"$C\" || fail \"$C not shown\"; fi\n",
    ),
    (
        "rmdir \"$S/h\" \"$S/w\" \"$S/k\" \"$S\" ||",
        "rmdir \"$S/h\" \"$S/w\" \"$S/k\" \"$S/c\" \"$S\" ||",
    ),
];

/// What [`ns_text`] changes with [`TASKS_SWITCH`] on, after
/// [`SKILLS_EDITS`] (if any), each found exactly once either way (a test
/// holds it): [`CONTROL_SOCKET`], the file alone, staged with the rest
/// before `/run` is hidden (skipped when no socket is there), moved back
/// into an otherwise empty `/run/lampos-control` and remounted read-only,
/// and its staging file removed. `admin.sock` is never named.
///
/// With [`NET_ENV`] `run` (a skill's run, in its own network namespace from
/// [`wrapper_text`]) it also: stages [`MODEL_SOCKET`], the file alone, and
/// shows it read-only the same way (missing, the run ends); brings `lo` up
/// with `ip` (`/usr/sbin/ip` or `/sbin/ip`; none, the run ends); and starts
/// [`RELAY`] `net-relay` in the background as the caller's own ids in a
/// nested user namespace (so it holds no capability over these mounts),
/// waiting for its `ready` line (none, the run ends). The PID namespace
/// ends the relay with Hermes. No other address is reachable: the
/// namespace has only `lo`. And it shows [`PUSHED`] read-only, staged and
/// moved back as the skills are (skipped when the folder is not there),
/// so a pushed skill's script is there for its run; Hermes started any
/// other way (Harness's panes) never sees it.
const TASKS_EDITS: [(&str, &str); 7] = [
    (
        "# where these mounts are locked. Any step failing ends it before Hermes runs.\n",
        "# where these mounts are locked. Any step failing ends it before Hermes runs.\n\
         # LAMP_SKILL_TASKS=on (#6306): it also shows lamp-control's socket /run/lampos-control/control.sock alone, read-only,\n\
         # so a skill's run reaches the lamp with its one-run token; nothing else of lamp-control's folder is shown.\n\
         # With LAMP_NS_NET=run (a skill's run, its own network namespace) it shows lamp-tasks' model.sock alone, read-only,\n\
         # brings lo up and starts lamp-tasks net-relay on 127.0.0.1:7741 to it: the run's only other way out.\n\
         # It also shows the skills the owner's computer pushed, /var/lib/lampOS/skills-dev, read-only, when there.\n",
    ),
    (
        " S=/tmp/.lampos-ns\n",
        " L=/run/lampos-control/control.sock B=/run/lampos-tasks/model.sock P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns\n",
    ),
    (
        "if [ -n \"$R\" ]; then touch \"$S/r\" && mount --bind \"$R\" \"$S/r\" || fail \"no $R\"; fi\n",
        "if [ -n \"$R\" ]; then touch \"$S/r\" && mount --bind \"$R\" \"$S/r\" || fail \"no $R\"; fi\n\
         if [ -S \"$L\" ]; then touch \"$S/l\" && mount --bind \"$L\" \"$S/l\" || fail \"no $L\"; else L=; fi\n\
         if [ \"${LAMP_NS_NET-}\" = run ]; then [ -S \"$B\" ] && touch \"$S/b\" && mount --bind \"$B\" \"$S/b\" || fail \"no $B\"; fi\n\
         if [ \"${LAMP_NS_NET-}\" = run ] && [ -d \"$P\" ]; then mkdir -m 0700 \"$S/p\" && mount --bind \"$P\" \"$S/p\" || fail \"no $P\"; else P=; fi\n",
    ),
    (
        "&& mount --move \"$S/r\" \"$R\" && ro \"$R\" || fail \"$R not shown\"; fi\n",
        "&& mount --move \"$S/r\" \"$R\" && ro \"$R\" || fail \"$R not shown\"; fi\n\
         if [ -n \"$L\" ]; then mkdir -p \"${L%/*}\" && touch \"$L\" && mount --move \"$S/l\" \"$L\" && ro \"$L\" || fail \"$L not shown\"; fi\n\
         if [ \"${LAMP_NS_NET-}\" = run ]; then mkdir -p \"${B%/*}\" && touch \"$B\" && mount --move \"$S/b\" \"$B\" && ro \"$B\" || fail \"$B not shown\"; fi\n\
         if [ -n \"$P\" ]; then mkdir -p \"$P\" && mount --move \"$S/p\" \"$P\" && ro \"$P\" || fail \"$P not shown\"; fi\n",
    ),
    (
        "rm -f \"$S/r\" &&",
        "rm -f \"$S/r\" \"$S/l\" \"$S/b\" && { [ -z \"$P\" ] || rmdir \"$S/p\"; } &&",
    ),
    (
        "unset LAMP_NS_UID LAMP_NS_GID\nexport HOME\n",
        "unset LAMP_NS_UID LAMP_NS_GID\nexport HOME\n\
         if [ \"${LAMP_NS_NET-}\" = run ]; then\n\
         \x20 I=; for p in /usr/sbin/ip /sbin/ip; do if [ -x \"$p\" ]; then I=$p; break; fi; done\n\
         \x20 [ -n \"$I\" ] || fail \"no ip: lo not up, the run has no way out\"\n\
         \x20 \"$I\" link set lo up || fail \"lo not up\"\n\
         \x20 [ -z \"$(\"$I\" -o link show | cut -d: -f2 | tr -d ' ' | grep -vx lo | grep -v '@NONE$')\" ] || fail \"a link besides lo: the run is not held off the network\"\n\
         \x20 M=$(/usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" /opt/lampOS/tasks/current/lamp-tasks net-relay --listen 127.0.0.1:7741 --to \"$B\" &)\n\
         \x20 [ \"$M\" = ready ] || fail \"no model relay on 127.0.0.1:7741\"\n\
         fi\n\
         unset LAMP_NS_NET\n",
    ),
    (
        "U=$LAMP_NS_UID G=$LAMP_NS_GID HOME=$H\n",
        concat!(
            "U=$LAMP_NS_UID G=$LAMP_NS_GID HOME=$H\n",
            "if [ -n \"${LAMP_NS_HOME-}\" ]; then\n  F=${LAMP_NS_HOME%/home} X=/tmp/.lampos-mk\n  [ -d \"$LAMP_NS_HOME\" ] && [ -n \"$W\" ] && [ \"${F%/*}\" = \"$W/lamp\" ] || fail \"no $LAMP_NS_HOME\"\n  mkdir -m 0700 \"$X\" \"$X/f\" \"$X/c\" || fail \"no make staging\"\n  mount --bind \"$F\" \"$X/f\" || fail \"no $F\"\n  mount --bind \"$H/.hermes/hermes-agent\" \"$X/c\" || fail \"no Hermes checkout\"\n  mount -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$H/.hermes\" || fail \"$H/.hermes not hidden\"\n  mkdir \"$H/.hermes/hermes-agent\" && mount --move \"$X/c\" \"$H/.hermes/hermes-agent\" && ro \"$H/.hermes/hermes-agent\" || fail \"Hermes checkout not shown\"\n  ro \"$H/.hermes\"; ro \"$H\"; ro \"$W\"\n  mount -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$W/lamp\" || fail \"$W/lamp not hidden\"\n  mkdir \"$F\" && mount --move \"$X/f\" \"$F\" || fail \"$F not shown\"\n  ro \"$W/lamp\"\n  rmdir \"$X/f\" \"$X/c\" \"$X\" || fail \"make staging left\"\n  cd \"$F\" || fail \"$F not visible\"\n  HOME=$LAMP_NS_HOME\nfi\nunset LAMP_NS_HOME F X\n"
        ),
    ),
];

/// [`NS`]'s text for what it shows: [`NS_TEXT`] with [`SKILLS_EDITS`]
/// when `skills` ([`SKILLS_SWITCH`]), which also shows Hermes
/// [`SKILLS_SOCKET`], and [`TASKS_EDITS`] when `tasks` ([`TASKS_SWITCH`]),
/// which also shows [`CONTROL_SOCKET`] alone, each read-only; both off,
/// [`NS_TEXT`]. The switches live only in this text, which root writes
/// and `lampos` cannot change: the script reads nothing to decide them.
/// Last, the shared namespace piece's `PATH` ([`agent_ns::with_ns_path`],
/// #6530) goes before the line that runs Hermes, so Harness's hooks find
/// [`agent_ns::HOOK_SHIM`] as `harness`.
pub fn ns_text(skills: bool, tasks: bool) -> String {
    let mut text = NS_TEXT.to_owned();
    for (on, edits) in [(skills, &SKILLS_EDITS[..]), (tasks, &TASKS_EDITS[..])] {
        if on {
            for (from, to) in edits {
                text = text.replacen(from, to, 1);
            }
        }
    }
    agent_ns::with_ns_path(&text)
}

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
    /// written, the one-shot not started; stopped by lamp-update's watch
    /// ([`crate::hermes_schedule::Watch`]) as the voice's privacy reading
    /// was lost, or after it was frozen too long while the voice was not
    /// quiet.
    pub const ERRORS: [&str; 15] = [
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
        "privacy_lost",
        "interrupted",
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
    /// [`SKILLS_SWITCH`] on: the namespace script written with
    /// [`Shown::skills`]. Absent (an older round's ask) is off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    skills: bool,
    /// [`TASKS_SWITCH`] on: the namespace script written with
    /// [`Shown::tasks`]. Absent (an older round's ask) is off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    tasks: bool,
    /// [`Shown::one_path`]: the wrapper written as the one run path's.
    /// Absent (an older round's ask) is off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    one_path: bool,
}

impl Ask {
    fn shown(&self) -> Shown {
        Shown {
            skills: self.skills,
            tasks: self.tasks,
            one_path: self.one_path,
        }
    }
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

pub(crate) fn on(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

/// The entry points `lampos` reaches Hermes by, any of them there.
fn entry_points(root: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    [WRAPPER, NS, RUN_ENV]
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
pub(crate) fn holds(root: &Path, path: &str, text: &str) -> bool {
    let place = on(root, path);
    fs::symlink_metadata(&place).is_ok_and(|meta| meta.is_file())
        && fs::read(&place).is_ok_and(|bytes| bytes == text.as_bytes())
}

/// Hermes there as this build puts it, under `root`: its command, its
/// checkout at [`COMMIT`], and the wrapper and namespace script with this
/// build's text for `shown` ([`wrapper_for`], [`ns_for`]: with
/// [`Shown::one_path`], the one run path's wrapper and the one agent
/// namespace). Any of them
/// otherwise is Hermes missing, put there again: so turning
/// [`SKILLS_SWITCH`] or [`TASKS_SWITCH`] on or off has the script written
/// again.
pub fn present(root: &Path, shown: Shown) -> bool {
    fs::metadata(on(root, HERMES)).is_ok()
        && head(root).as_deref() == Some(COMMIT)
        && holds(root, WRAPPER, &wrapper_for(shown))
        && {
            let (ns, text) = ns_for(shown);
            holds(root, ns, &text)
        }
        // What Hermes's terminal sources for a skill run's SDK path (#6684).
        && holds(root, RUN_ENV, RUN_ENV_TEXT)
        // The build's own lampos SDK, one copy for every agent (#6507).
        && crate::sdk_tree::present(root)
        && agent_ns::hook_shim_there(root)
}

/// When `serve` starts: off, an earlier record and ask are taken away; on
/// with Hermes missing (as `shown` says, [`SKILLS_SWITCH`] and
/// [`TASKS_SWITCH`]) and no record, `absent`.
pub fn at_start(root: &Path, records: &Path, switch: bool, shown: Shown, now_unix_ms: u64) {
    if !switch {
        for name in [RECORD, ASK] {
            let _ = fs::remove_file(records.join(name));
        }
        return;
    }
    if !present(root, shown) && record(records).is_none() {
        write(records, HermesInstall::new("absent", None, now_unix_ms));
    }
}

fn ask(records: &Path, remove: bool, shown: Shown) -> io::Result<()> {
    let text = serde_json::to_vec(&Ask {
        version: 1,
        remove,
        skills: shown.skills,
        tasks: shown.tasks,
        one_path: shown.one_path,
    })
    .map_err(io::Error::other)?;
    write_whole(&records.join(ASK), &text, 0o600)
}

/// After a round that reached the channel, with the switch on, and after
/// Harness's own [`harness_install::after_round`]: Hermes there is recorded
/// `installed` (once) and nothing runs. Missing, with no try under way and
/// no recent failure, it waits for its `window`
/// ([`crate::hermes_schedule::window`]): closed, one line says why, and an
/// install [`ASK`] an earlier round left is taken back, so a one-shot
/// started for Harness alone does not run Hermes's install now. Open,
/// [`ASK`] is written; then with Harness there, `installing` is recorded
/// and Harness's one-shot started without waiting (a unit not started is
/// `not_started`), and with Harness missing nothing more: the one-shot
/// Harness's round starts reads the ask after Harness. Hermes is there
/// only with the namespace script [`ns_text`] gives for `shown`
/// ([`SKILLS_SWITCH`], [`TASKS_SWITCH`]), and the ask says so. What was recorded is returned, none when nothing
/// changed.
pub fn after_round(
    root: &Path,
    records: &Path,
    shown: Shown,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    window: Result<crate::hermes_schedule::Window, String>,
    say: &mut dyn FnMut(&str),
) -> Option<HermesInstall> {
    let before = record(records);
    if present(root, shown) {
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
        // A tap from the app (#6240) is a deliberate try again: a recent
        // failure does not hold it.
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
                say("Hermes's install asked earlier is taken back: not its window");
            }
            say(&format!(
                "Hermes {VERSION} is not set up yet: it waits, {why}"
            ));
            return None;
        }
    };
    // Turns in the one-shot (#6226): OpenClaw's install under way holds
    // Hermes's to a later round, so no ask is left for a one-shot past it.
    if crate::openclaw_install::under_way(records, now_unix_ms) {
        say("Hermes waits its turn: OpenClaw's install is under way in Harness's one-shot");
        return None;
    }
    say(&format!("Hermes may be installed now: {}", window.name()));
    if let Err(error) = ask(records, false, shown) {
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

/// Failures tried again at the next round whose window is open, not
/// [`RETRY_AFTER`] later: lamp-update's own (`serve`, `not_started`), and
/// an install lamp-update's watch stopped (`privacy_lost`, `interrupted`),
/// which a quieter window may finish.
pub const RETRY_IN_NEXT_WINDOW: [&str; 4] = ["serve", "not_started", "privacy_lost", "interrupted"];

/// An install [`ASK`] taken back (an ask to remove is kept): whether one
/// was there.
pub fn withhold(records: &Path) -> bool {
    let Ok(bytes) = fs::read(records.join(ASK)) else {
        return false;
    };
    let asked = serde_json::from_slice::<Ask>(&bytes).unwrap_or_default();
    !asked.remove && fs::remove_file(records.join(ASK)).is_ok()
}

/// Whether a Hermes install is under way as the record says: what the
/// watch looks at before it asks systemd.
pub fn installing(records: &Path) -> bool {
    record(records).is_some_and(|said| said.install == "installing")
}

/// The watch's `act` ([`crate::hermes_schedule::Watch::look`]) carried out
/// on Harness's one-shot, only while systemd says it runs (`activating`):
/// frozen (`systemctl freeze`) or thawed, or stopped (thawed first) and
/// recorded `failed` with its error, with its [`ASK`] taken back. A freeze
/// or thaw that systemd refuses stops the install, so the one-shot is
/// never left frozen.
pub fn carry_out(
    act: &crate::hermes_schedule::Act,
    records: &Path,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> Carried {
    carry_out_for(&TURN, &[], act, records, systemd, now_unix_ms, say)
}

/// One engine's install in Harness's one-shot, as the watch carries out
/// its acts (#6226): Hermes's ([`TURN`]) and OpenClaw's
/// ([`crate::openclaw_install::TURN`]) take turns in the same one-shot,
/// frozen and thawed as one.
pub struct Turn {
    /// The engine, as said.
    pub name: &'static str,
    /// Whether its record says an install is under way.
    pub installing: fn(&Path) -> bool,
    /// Its install recorded `failed` with the error, its ask taken away.
    pub stopped: fn(&Path, &str, u64),
}

/// Hermes's [`Turn`].
pub const TURN: Turn = Turn {
    name: "Hermes",
    installing,
    stopped: |records, error, now_unix_ms| {
        stopped(records, error, now_unix_ms);
    },
};

/// [`carry_out`] for `turn`'s install; a stop also stops each of `also`
/// whose record says it is under way (its turn in the same one-shot, which
/// is stopped whole), recorded `failed` with the same error.
pub fn carry_out_for(
    turn: &Turn,
    also: &[&Turn],
    act: &crate::hermes_schedule::Act,
    records: &Path,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> Carried {
    use crate::hermes_schedule::Act;
    let unit = harness_install::UNIT;
    let name = turn.name;
    if *act == Act::Nothing {
        return Carried::Done;
    }
    let runs = systemd.states(&[unit.to_owned()]).is_ok_and(|states| {
        states
            .iter()
            .any(|state| state.unit == unit && state.active == "activating")
    });
    if !runs {
        return Carried::NotRunning;
    }
    let (error, why) = match act {
        Act::Nothing => return Carried::Done,
        Act::Freeze(why) => match systemd.freeze_harness_unit() {
            Ok(()) => {
                say(&format!(
                    "{name}'s install is frozen while the voice is not quiet: {why}"
                ));
                return Carried::Done;
            }
            Err(refused) => ("interrupted", format!("{unit} not frozen: {refused}")),
        },
        Act::Thaw => match systemd.thaw_harness_unit() {
            Ok(()) => {
                say(&format!(
                    "{name}'s install goes on: the voice is quiet again"
                ));
                return Carried::Done;
            }
            Err(refused) => ("interrupted", format!("{unit} not thawed: {refused}")),
        },
        Act::Stop { error, why } => (*error, why.clone()),
    };
    let _ = systemd.thaw_harness_unit();
    if let Err(refused) = systemd.stop(unit) {
        say(&format!("{unit} not stopped: {refused}"));
    }
    (turn.stopped)(records, error, now_unix_ms);
    say(&format!(
        "{name}'s install is stopped ({error}): {why}; tried again in a later window"
    ));
    for other in also {
        if (other.installing)(records) {
            (other.stopped)(records, error, now_unix_ms);
            say(&format!(
                "{}'s install, in the same one-shot, is stopped too ({error}); tried again in a later window",
                other.name
            ));
        }
    }
    Carried::Stopped
}

/// What [`carry_out`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carried {
    /// Done as asked, or nothing asked.
    Done,
    /// The one-shot does not run (`activating`) as systemd says: nothing
    /// done, and the watch starts again once it does.
    NotRunning,
    /// The install stopped and recorded `failed`.
    Stopped,
}

/// An install the watch stopped: recorded `failed` with `error`, and its
/// [`ASK`] taken away, as the one-shot was killed before it could.
/// What it left in [`PLACE`] is taken up by the next try, as a cut-off
/// try's is; the entry points are written only as an install ends.
pub fn stopped(records: &Path, error: &str, now_unix_ms: u64) -> HermesInstall {
    let _ = fs::remove_file(records.join(ASK));
    write(records, failed(now_unix_ms, error))
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
    let started = ask(records, true, Shown::default())
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
/// install ([`harness_install::install_now`]) whenever a round's ask is
/// there (#6275): it installs a missing Harness, moves one there when the
/// ask says so (#5957), and records one there as it is; then Hermes's
/// ([`install_now`]) when a round asked for it, whatever Harness's did, then
/// OpenClaw's ([`crate::openclaw_install::install_now`], #6226) the same way.
/// With no Harness ask (its switch off), Harness's record is left as it is. An
/// error only for Harness: a Hermes failure is said and recorded, never
/// the unit's.
pub fn one_shot(
    root: &Path,
    records: &Path,
    run: &mut Runner,
    now_unix_ms: &mut dyn FnMut() -> u64,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let harness = records
        .join(harness_install::ASK)
        .exists()
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
    // OpenClaw after Hermes (#6226), whatever Hermes's did; never the
    // unit's failure either.
    match crate::openclaw_install::install_now(root, records, run, now_unix_ms, say) {
        Ok(Some(said)) => {
            if let Some(error) = said.error {
                say(&format!("OpenClaw is not installed: {error}"));
            }
        }
        Ok(None) => {}
        Err(why) => say(&format!("OpenClaw is not installed: {why}")),
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
        let said = steps(root, records, run, now_unix_ms, say, kit, asked.shown());
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
/// `lampos` finds no `hermes`, then the shared hook stand-in unless
/// another engine's namespace script still uses it. Hermes's folder and
/// Hermes in it stay.
fn remove_entry_points(root: &Path, say: &mut dyn FnMut(&str)) {
    for path in [WRAPPER, NS, RUN_ENV] {
        match fs::remove_file(on(root, path)) {
            Ok(()) => say(&format!("{path} taken out")),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => say(&format!("{path} not taken out: {error}")),
        }
    }
    agent_ns::remove_hook_shim(root, &NS_SCRIPTS, say);
}

/// Every engine's namespace script: the shared hook stand-in
/// ([`agent_ns::HOOK_SHIM`]) stays while any of them is there, the one
/// every other engine's wrapper runs ([`agent_ns::wrapper::NS`], #6510)
/// included.
pub(crate) const NS_SCRIPTS: [&str; 3] = [NS, crate::openclaw_install::NS, agent_ns::wrapper::NS];

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
    shown: Shown,
) -> HermesInstall {
    if present(root, shown) && kit.commit == COMMIT {
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
             Hermes would see the lamp's services' files, so nothing is installed"
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
    if let Err(why) = write_entry_points(root, shown) {
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
    let shown = match (shown.skills, shown.tasks) {
        (false, false) => "its own folder, its work folder and the skills",
        (true, false) => "its own folder, its work folder, the skills and lamp-skills' socket",
        (false, true) => "its own folder, its work folder, the skills and lamp-control's socket",
        (true, true) => {
            "its own folder, its work folder, the skills, lamp-skills' socket and lamp-control's socket"
        }
    };
    say(&format!(
        "Hermes {VERSION} ({}) installed for {USER} at {HERMES}; Harness runs it through {WRAPPER}, in a namespace that shows it only {shown}; no service, nothing at boot",
        kit.commit
    ));
    write(records, HermesInstall::installed(now_unix_ms()))
}

/// `user`'s group id, from `id -g`, through `run`.
pub(crate) fn gid_of(run: &mut Runner, user: &str) -> Result<String, String> {
    let gid = run(&["id".to_owned(), "-g".to_owned(), user.to_owned()])?
        .trim()
        .to_owned();
    if gid.is_empty() || !gid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("id -g {user} gave no group id"));
    }
    Ok(gid)
}

/// The owner's uid of `path` under `root`, not followed through a link.
pub(crate) fn owner(root: &Path, path: &str) -> Option<String> {
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
pub(crate) fn free_kb(run: &mut Runner) -> Option<u64> {
    let text = run(&["df", "-Pk", "/var/lib"].map(str::to_owned)).ok()?;
    text.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()
}

/// The KiB of memory available, from [`MEMINFO`]'s `MemAvailable` line;
/// none when the file or the line is not read.
pub(crate) fn available_kb(root: &Path) -> Option<u64> {
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
/// installed: a user, mount and PID namespace with its own `/proc`, with
/// `lampos` as its root (as [`WRAPPER_TEXT`] makes it), then a
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
        "--pid",
        "--kill-child",
        "--mount-proc",
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

/// The namespace script ([`ns_for`] for `shown`), the shared hook
/// stand-in ([`agent_ns::write_hook_shim`]), then the wrapper
/// ([`wrapper_for`]): root's, 0755, each written whole beside and renamed
/// in (never through a link), so `lampos` never finds a wrapper before
/// what it runs. With [`Shown::one_path`], [`NS`] is taken out after: the
/// wrapper runs the one namespace script every agent runs.
fn write_entry_points(root: &Path, shown: Shown) -> Result<(), String> {
    // The build's own lampos SDK first (#6507): the one read-only copy a skill's script imports.
    crate::sdk_tree::write(root)?;
    // What Hermes's terminal sources for a skill run's SDK path (#6684): 0644, never run.
    let place = on(root, RUN_ENV);
    write_whole(&place, RUN_ENV_TEXT.as_bytes(), 0o644)
        .map_err(|error| format!("{RUN_ENV}: {error}"))?;
    let (ns_path, ns) = ns_for(shown);
    let wrapper = wrapper_for(shown);
    for (path, text) in [(ns_path, ns.as_str()), (WRAPPER, wrapper.as_str())] {
        let place = on(root, path);
        if let Some(folder) = place.parent()
            && !folder.is_dir()
        {
            return Err(format!("{}: no such folder", folder.display()));
        }
        write_whole(&place, text.as_bytes(), 0o755).map_err(|error| format!("{path}: {error}"))?;
        if path == ns_path {
            agent_ns::write_hook_shim(root)?;
        }
    }
    // The one run path: nothing runs Hermes's own namespace script any more.
    if shown.one_path {
        match fs::remove_file(on(root, NS)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{NS}: {error}")),
        }
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
    lock_named(records, LOCK, "Hermes")
}

/// The lock file `name` in `records`, held while `engine`'s install runs
/// (Hermes's [`LOCK`], OpenClaw's own, #6226); one held already is refused.
pub(crate) fn lock_named(records: &Path, name: &str, engine: &str) -> Result<fs::File, String> {
    fs::create_dir_all(records).map_err(|error| format!("{}: {error}", records.display()))?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(records.join(name))
        .map_err(|error| format!("{name}: {error}"))?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
        |_| {
            io::Error::new(
                ErrorKind::WouldBlock,
                format!("another {engine} install is at work"),
            )
            .to_string()
        },
    )?;
    Ok(file)
}

#[cfg(test)]
mod tests;
