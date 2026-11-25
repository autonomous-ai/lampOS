//! Hermes's API gateway on the lamp (Hermes direct; docs/install.md,
//! "Hermes on the lamp"): [`UNIT`] runs `hermes gateway run` with only its
//! `api_server` platform, on loopback ([`HOST`]:[`PORT`]) with a key of its
//! own ([`KEY`]), so lamp-tasks hands the lamp's own tasks to one kept
//! Hermes session over the Runs API instead of a Harness pane per task.
//! The counterpart of OpenClaw's gateway
//! ([`crate::openclaw_install::UNIT`]), in Hermes's own namespace and user.
//!
//! **Behind [`SWITCH`].** `LAMP_HERMES_GATEWAY=on` exactly, in lamp-update's
//! environment or the carried `/etc/lampOS/update.env`, on in the product
//! by that file (with lamp-tasks' `LAMP_TASKS_HERMES_DIRECT` on in its
//! product unit). Off, nothing of this is written,
//! and a unit an earlier round wrote is disabled, stopped and taken out
//! ([`after_round_off`]).
//!
//! **One agent gateway per lamp.** With the switch on, the gateway is
//! enabled and started only while lamp-tasks says Hermes is the lamp's
//! default agent ([`gateway_wanted`]), disabled and stopped while it says
//! another; unknown, it is left as it is. A round looks once, and `serve`
//! every [`crate::openclaw_install::KEEP_EVERY`] between rounds
//! ([`keep_between_rounds`]), both through [`crate::gateway_keeper`].
//! OpenClaw's gateway follows the default with the switch on too (main:
//! `LAMP_OPENCLAW_GATEWAY_BY_DEFAULT` read as on), so the two never run
//! side by side: the keeper reads the default once for both, stops the
//! unwanted gateway first, and starts the wanted one only once the other
//! is down.
//!
//! **As `lampos`, in Hermes's namespace.** The unit starts [`NS`] in a new
//! user, mount and PID namespace (`unshare`, as Hermes's wrapper does) with
//! `HERMES_HOME` Hermes's own ([`crate::hermes_install::HERMES_HOME`]).
//! [`NS`] is Hermes's namespace script ([`crate::hermes_install::ns_text`],
//! the same allow-list and switches) with [`NS_EDITS`]: the ids read from
//! the namespace's own maps; lamp-cloud's forwarder token and the gateway's
//! key read before `/var/lib` is hidden, and given only as
//! `LAMP_HERMES_TOKEN` and `API_SERVER_KEY`; lamp-tasks' skill-start
//! socket folder ([`START_SOCKET_DIR`]) shown read-only; the launcher
//! ([`SKILL_START`]) first on `PATH` as `skill-start`, through a link on
//! the read-only `/run`; and the gateway run as the shell's child, so a
//! stop's SIGTERM is passed on (#8289 on OpenClaw). Nothing else runs
//! there. As OpenClaw's gateway, and as the owner asks (Hermes behaves as
//! OpenClaw direct): no `--net`, so the gateway, its tasks and skills have
//! the lamp's network; skills reach the lamp's services only through
//! skill-start grants; and with the tasks switch on the pushed skills'
//! folder is shown whenever it is there. Hermes's Harness path keeps its
//! per-run `--net` unchanged: its wrapper and script are not touched. The key's folder ([`KEY_DIR`], the unit's `StateDirectory`,
//! `lampos`'s 0700) is under the hidden `/var/lib`: Hermes's tools never
//! see the file, only the gateway's own environment has the key.
//! lamp-tasks runs as `lampos` too (its product unit, which hides no part
//! of it), so it reads the key there. lamp-tasks' work folder
//! ([`hermes_install::WORK`], where a direct make writes its folder) is
//! shown whole and read-write, as to OpenClaw's gateway: only a skill's run
//! of its own (`LAMP_NS_NET=run`, never set here) is narrowed.
//!
//! **Non-interactive approvals.** lamp-tasks keeps Hermes's
//! `approvals.mode` `manual` (`lamp_tasks::hermes_config::APPROVALS`): on
//! the Runs API a dangerous command raises `approval.request`, which
//! lamp-tasks answers at once (`once` for the launcher alone, else `deny`).
//! `approvals.unattended_mode` (Hermes's default `deny`) covers a turn with
//! no client listening. `HERMES_YOLO_MODE` is unset in [`NS`].
//!
//! **Ready.** The unit is `active` only once `GET /health` (no key needed)
//! answers 200 ([`UNIT_TEXT`]'s `ExecStartPost`), so `ActiveState=active`
//! means a gateway that takes runs.
//!
//! **Written by whom.** The unit by a round (its sandbox writes
//! `/etc/systemd/system`); [`NS`] by Harness's one-shot as root, asked by a
//! round through Hermes's ask ([`crate::hermes_install::ask_gateway`]) when
//! Hermes is there and [`NS`] is not as this build writes it ([`present`]).
use crate::{
    hermes_install::{self, Shown, on},
    openclaw_install::{self, Gateway, Said},
    records::write_whole,
    world::Systemd,
};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// The switch, in lamp-update's environment or the carried
/// `/etc/lampOS/update.env`: `on` exactly.
pub const SWITCH: &str = "LAMP_HERMES_GATEWAY";
/// The gateway's unit: written by a round ([`unit_text`]), enabled
/// ([`WANTS`]) and started while Hermes is the lamp's default agent.
pub const UNIT: &str = "lamp-hermes-gateway.service";
/// What `systemctl enable` makes of the unit's `WantedBy`.
pub const WANTS: &str = "/etc/systemd/system/multi-user.target.wants/lamp-hermes-gateway.service";
/// The gateway's namespace script: [`ns_text`], root's, 0755, written by
/// Harness's one-shot beside Hermes's own.
pub const NS: &str = "/usr/local/lib/lampos/hermes-gateway-ns";
/// The address the API server binds: loopback only (Hermes's
/// `API_SERVER_HOST`, also its default).
pub const HOST: &str = "127.0.0.1";
/// Its port (Hermes's `API_SERVER_PORT`, also its default 8642).
pub const PORT: u16 = 8642;
/// The readiness check: Hermes's `GET /health`, no key needed.
pub const HEALTH_URL: &str = "http://127.0.0.1:8642/health";
/// The key's folder: the unit's `StateDirectory` (`lampos`'s, 0700), under
/// `/var/lib`, which [`NS`] hides from Hermes.
pub const KEY_DIR: &str = "/var/lib/lampos-hermes-gateway";
/// The gateway's key (`API_SERVER_KEY`, Bearer): 64 hex digits, no
/// newline, made once by the unit's `ExecStartPre`, `lampos`'s, 0600.
/// lamp-tasks (as `lampos`) reads it to call the gateway.
pub const KEY: &str = "/var/lib/lampos-hermes-gateway/api-key";
/// lamp-tasks' skill-start socket folder, shown read-only in [`NS`]: the
/// same folder as OpenClaw's gateway's.
pub const START_SOCKET_DIR: &str = openclaw_install::START_SOCKET_DIR;
/// The skill-start launcher: the same file as OpenClaw's, kept by
/// OpenClaw's one-shot with the one run path on.
pub const SKILL_START: &str = openclaw_install::SKILL_START;
/// The folder [`NS`] makes on its read-only `/run`, first on the gateway's
/// `PATH`: only a link `skill-start` to [`SKILL_START`].
pub const LAUNCHER_DIR: &str = "/run/lampos-gateway-bin";

/// Hermes's own run line in its namespace script, which [`NS_EDITS`]
/// replace with [`GATEWAY_RUN`].
const RUN_LINE: &str = "exec /usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" /var/lib/lampos-hermes/.local/bin/hermes \"$@\"\n";

/// The gateway's run (after the `PATH` lines), in place of [`RUN_LINE`]:
/// only `gateway run …`; [`LAUNCHER_DIR`] first on `PATH`; Hermes started
/// as this shell's child, so this shell stays the PID namespace's PID 1 and
/// passes a stop's SIGTERM on (the kernel drops a signal from outside to a
/// PID 1 with no handler for it, and Python has none until the gateway has
/// loaded: #8289 on OpenClaw), exiting as Hermes does, 0 when it ended on
/// that SIGTERM.
pub const GATEWAY_RUN: &str = "# The gateway alone runs here, with skill-start first on its PATH. Hermes runs as this shell's child, not as the
# PID namespace's PID 1, which drops a signal from outside that it has no handler for (#8289 on OpenClaw): this shell
# passes a stop's SIGTERM on and exits as the gateway does, 0 when it ended on that SIGTERM.
[ \"${1-}\" = gateway ] && [ \"${2-}\" = run ] || fail \"only Hermes's gateway runs here\"
PATH=/run/lampos-gateway-bin:$PATH
export PATH
t=
trap 't=TERM' TERM
/usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" /var/lib/lampos-hermes/.local/bin/hermes \"$@\" &
c=$!
trap 't=TERM; kill -TERM \"$c\" 2>/dev/null' TERM
[ -z \"$t\" ] || kill -TERM \"$c\" 2>/dev/null
while wait \"$c\"; s=$?; [ \"$s\" -gt 128 ] && kill -0 \"$c\" 2>/dev/null; do :; done
if [ \"$s\" -gt 128 ]; then wait \"$c\"; r=$?; [ \"$r\" = 127 ] || s=$r; fi
if [ \"$s\" = 143 ] && [ -n \"$t\" ]; then s=0; fi
exit \"$s\"
";

/// Hermes's tasks-switch line that shows the pushed skills only to a
/// skill's run of its own (`LAMP_NS_NET=run`), and the gateway's, which
/// shows them whenever they are there.
const PUSHED_ONLY_IN_A_RUN: &str =
    "if [ \"${LAMP_NS_NET-}\" = run ] && [ -d \"$P\" ]; then mkdir -m 0700";
const PUSHED_ALWAYS: &str = "if [ -d \"$P\" ]; then mkdir -m 0700";

/// What [`ns_text`] changes in Hermes's namespace script, in order, each
/// found exactly once whatever the switches (a test holds it).
pub const NS_EDITS: [(&str, &str); 9] = [
    (
        "# lampOS: Hermes on the lamp, inside the namespace /usr/local/bin/hermes makes (#5978; lampOS/docs/install.md,\n",
        "# lampOS: Hermes's API gateway (lamp-hermes-gateway.service, Hermes direct). It reads the forwarder token and its key\n\
         # before /var/lib is hidden, shows lamp-tasks' skill-start socket folder /run/lampos-skill-start read-only and puts\n\
         # skill-start first on PATH; the rest as Hermes's own, inside the namespace its unit makes (#5978; lampOS/docs/install.md,\n",
    ),
    (
        "[ -n \"${LAMP_NS_UID-}\" ] && [ -n \"${LAMP_NS_GID-}\" ] || exit 1\n",
        "read -r _ LAMP_NS_UID _ < /proc/self/uid_map && read -r _ LAMP_NS_GID _ < /proc/self/gid_map || exit 1\n\
         unset HERMES_YOLO_MODE LAMP_NS_HOME LAMP_NS_NET LAMP_NS_EGRESS LAMP_CONTROL_TOKEN LAMP_CONTROL_SOCKET\n\
         [ -n \"${LAMP_NS_UID-}\" ] && [ -n \"${LAMP_NS_GID-}\" ] || exit 1\n",
    ),
    (
        "S=/tmp/.lampos-ns\n",
        "S=/tmp/.lampos-ns\nQ=/run/lampos-skill-start\n",
    ),
    (
        "mount -t tmpfs -o size=256m,mode=1777,nosuid,nodev none /tmp || fail \"no /tmp\"\n",
        "T=$(cat /var/lib/lampos-cloud/hermes-token 2>/dev/null) && [ -n \"$T\" ] || fail \"no forwarder token at /var/lib/lampos-cloud/hermes-token\"\n\
         A=$(cat /var/lib/lampos-hermes-gateway/api-key 2>/dev/null) && [ ${#A} -ge 32 ] || fail \"no key at /var/lib/lampos-hermes-gateway/api-key\"\n\
         LAMP_HERMES_TOKEN=$T API_SERVER_KEY=$A\n\
         export LAMP_HERMES_TOKEN API_SERVER_KEY\n\
         unset T A\n\
         mount -t tmpfs -o size=256m,mode=1777,nosuid,nodev none /tmp || fail \"no /tmp\"\n",
    ),
    (
        "if [ -d \"$K\" ]; then mount --bind \"$K\" \"$S/k\" || fail \"no $K\"; else K=; fi\n",
        "if [ -d \"$K\" ]; then mount --bind \"$K\" \"$S/k\" || fail \"no $K\"; else K=; fi\n\
         if [ -d \"$Q\" ]; then mkdir -m 0700 \"$S/q\" && mount --bind \"$Q\" \"$S/q\" || fail \"no $Q\"; else Q=; fi\n",
    ),
    (
        "if [ -n \"$K\" ]; then mkdir -p \"$K\" && mount --move \"$S/k\" \"$K\" && ro \"$K\" || fail \"$K not shown\"; fi\n",
        "if [ -n \"$K\" ]; then mkdir -p \"$K\" && mount --move \"$S/k\" \"$K\" && ro \"$K\" || fail \"$K not shown\"; fi\n\
         if [ -n \"$Q\" ]; then mkdir -p \"$Q\" && mount --move \"$S/q\" \"$Q\" && ro \"$Q\" || fail \"$Q not shown\"; fi\n",
    ),
    (
        "rmdir \"$S/h\" \"$S/w\" \"$S/k\"",
        "{ [ -z \"$Q\" ] || rmdir \"$S/q\"; } && rmdir \"$S/h\" \"$S/w\" \"$S/k\"",
    ),
    (
        "mount -o remount,bind,ro,nosuid,nodev,noexec /run || fail \"/run not read-only\"\n",
        "mkdir -m 0755 /run/lampos-gateway-bin && ln -s /usr/local/lib/lampos/skill-start /run/lampos-gateway-bin/skill-start || fail \"no skill-start for the gateway\"\n\
         mount -o remount,bind,ro,nosuid,nodev,noexec /run || fail \"/run not read-only\"\n",
    ),
    (RUN_LINE, GATEWAY_RUN),
];

/// [`NS`]'s text for `shown`: Hermes's ([`hermes_install::ns_text`] with
/// `shown`'s skills and tasks switches) with [`NS_EDITS`], its messages as
/// `hermes-gateway-ns`, a voice make's home lines taken out (a make never
/// runs on the gateway), and with the tasks switch on, the wait for
/// lamp-control's socket before it is bound
/// ([`openclaw_install::WAIT_FOR_CONTROL`]), as OpenClaw's gateway has.
pub fn ns_text(shown: Shown) -> String {
    let mut text = hermes_install::ns_text(shown.skills, shown.tasks);
    if shown.tasks {
        text = text.replacen(hermes_install::NS_HOME_LINE, "", 1);
        // As OpenClaw's gateway (its RUNS_EDITS): the pushed skills' folder
        // shown whenever it is there, as a skill's run is a turn on the
        // gateway, which never sets LAMP_NS_NET.
        text = text.replacen(PUSHED_ONLY_IN_A_RUN, PUSHED_ALWAYS, 1);
        let bind = "if [ -S \"$L\" ]; then";
        text = text.replacen(
            bind,
            &format!("{}{bind}", openclaw_install::WAIT_FOR_CONTROL),
            1,
        );
    }
    for (from, to) in NS_EDITS {
        text = text.replacen(from, to, 1);
    }
    text.replace("hermes-ns: ", "hermes-gateway-ns: ").replace(
        "only inside the namespace /usr/local/bin/hermes makes",
        "only inside the namespace lamp-hermes-gateway.service makes",
    )
}

/// The unit's text. As OpenClaw's gateway's: `lampos`, always started again
/// when it exits (Hermes's in-chat restart exits back to its supervisor,
/// `--external-supervisor`), given up on after five starts in ten minutes,
/// below the lamp's services in CPU. Its memory guard is a sane value, not
/// measured on a lamp yet. `ExecStartPre` makes the key once;
/// `ExecStartPost` holds the unit `activating` until `/health` answers
/// (150 s at most, else the start fails and is tried again).
/// lamp-tasks' skill-start socket folder is made at each start and kept
/// past a stop (until a reboot), as OpenClaw's gateway's unit keeps it.
/// With the tasks switch on it starts after lamp-control, as OpenClaw's.
pub const UNIT_TEXT: &str = "\
# lamp-hermes-gateway: Hermes's API gateway on the lamp, which lamp-tasks hands the lamp's own tasks to (Hermes direct;
# lampOS/docs/install.md, \"Hermes on the lamp\"). Written by a round of lamp-update with LAMP_HERMES_GATEWAY on; enabled and
# started only while Hermes is the lamp's default agent, disabled and stopped otherwise (one agent gateway per lamp).
# It runs only in its namespace (/usr/local/lib/lampos/hermes-gateway-ns: its own user, mount and PID namespace, as
# Hermes's), as lampos, with only the api_server platform on 127.0.0.1:8642 and its own key; active once /health answers.
[Unit]
Description=lampOS Hermes API gateway (lamp-hermes-gateway)
After=network-online.target lamp-cloud.service
StartLimitIntervalSec=600
StartLimitBurst=5

[Service]
Type=simple
User=lampos
Group=lampos
Environment=HOME=/var/lib/lampos-hermes HERMES_HOME=/var/lib/lampos-hermes/.hermes API_SERVER_HOST=127.0.0.1 API_SERVER_PORT=8642 PATH=/usr/bin:/bin
WorkingDirectory=/var/lib/lampos-hermes
# The gateway's key, lampos's 0700, under /var/lib, which its namespace hides from Hermes.
StateDirectory=lampos-hermes-gateway
StateDirectoryMode=0700
# The folder of lamp-tasks' socket for /usr/local/lib/lampos/skill-start, which hermes-gateway-ns shows read-only; kept
# past a stop, so lamp-tasks' socket in it outlives a gateway restart.
RuntimeDirectory=lampos-skill-start
RuntimeDirectoryMode=0755
RuntimeDirectoryPreserve=yes
ExecStartPre=/bin/sh -c 'K=/var/lib/lampos-hermes-gateway/api-key; [ -f \"$$K\" ] && [ ! -L \"$$K\" ] && [ -s \"$$K\" ] || { umask 077 && rm -f -- \"$$K\" && od -An -N32 -tx1 /dev/urandom | tr -dc 0-9a-f > \"$$K.new\" && mv -f -- \"$$K.new\" \"$$K\"; }'
ExecStart=/usr/bin/unshare --map-root-user --mount --pid --fork --kill-child --mount-proc --propagation private /usr/local/lib/lampos/hermes-gateway-ns gateway run --external-supervisor
ExecStartPost=/bin/sh -c 'i=0; until /usr/bin/curl -fs -o /dev/null -m 2 http://127.0.0.1:8642/health; do i=$$((i + 1)); [ \"$$i\" -lt 150 ] || exit 1; sleep 1; done'
TimeoutStartSec=180
Restart=always
RestartSec=10
# Hermes's gateway exits 1 on the SIGTERM of a stop (lamp-ee17, 24 Nov): a stop is no failure, so a later start
# (the default agent switched back to Hermes) finds it inactive, not failed.
SuccessExitStatus=1
StandardInput=null
SyslogIdentifier=lamp-hermes-gateway
Nice=10
CPUWeight=20
CPUQuota=100%
# Not measured on a lamp yet: a hard guard only, no throttle (#6226 on OpenClaw: a throttle stalled its gateway).
MemoryMax=1500M
TasksMax=256
NoNewPrivileges=yes

[Install]
WantedBy=multi-user.target
";

/// The line of [`unit_text`] (tasks on) that restarts the gateway with
/// lamp-control (#8320, as OpenClaw's).
const PART_OF: &str = "PartOf=lamp-control.service";

/// The unit's text for `shown`: [`UNIT_TEXT`]; with the tasks switch on,
/// [`openclaw_install::CONTROL_LINES`]: after lamp-control (and wanting
/// it), so the namespace's view of its socket is made once it is there,
/// and `PartOf=` it (#8320): the namespace binds lamp-control's socket
/// file once, at the gateway's start, and each lamp-control start makes it
/// anew (a new inode), so a lamp-control restart (an update round's
/// switch, the keeper, its own `Restart=`) restarts the gateway too. A
/// lamp-control stop stops it; [`keep_between_rounds`] starts it again
/// within [`openclaw_install::KEEP_EVERY`] while Hermes is the default.
pub fn unit_text(shown: Shown) -> String {
    if !shown.tasks {
        return UNIT_TEXT.to_owned();
    }
    UNIT_TEXT.replacen(
        "After=network-online.target lamp-cloud.service\n",
        openclaw_install::CONTROL_LINES,
        1,
    )
}

/// The unit's path under `root`.
fn unit_path(root: &Path) -> PathBuf {
    root.join(crate::writer_unit::FOLDER).join(UNIT)
}

/// [`NS`] there as this build writes it for `shown`: a regular file, never
/// followed through a link.
pub fn present(root: &Path, shown: Shown) -> bool {
    hermes_install::holds(root, NS, &ns_text(shown))
}

/// [`NS`] written for `shown`, root's, 0755, whole beside and renamed in:
/// Harness's one-shot's, asked by [`hermes_install::ask_gateway`].
pub fn write_ns(root: &Path, shown: Shown) -> Result<(), String> {
    let place = on(root, NS);
    if let Some(folder) = place.parent()
        && !folder.is_dir()
    {
        return Err(format!("{}: no such folder", folder.display()));
    }
    write_whole(&place, ns_text(shown).as_bytes(), 0o755).map_err(|error| format!("{NS}: {error}"))
}

/// [`NS`] taken out, with Hermes's entry points.
pub fn remove_ns(root: &Path, say: &mut dyn FnMut(&str)) {
    match fs::remove_file(on(root, NS)) {
        Ok(()) => say(&format!("{NS} taken out")),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => say(&format!("{NS} not taken out: {error}")),
    }
}

/// The rule: wanted while lamp-tasks says Hermes is the lamp's default
/// agent; not wanted while it says another; unknown (no answer, or no
/// default said), so a guess never starts or stops it.
pub fn gateway_wanted(said: Option<Said>) -> Gateway {
    match said.and_then(|said| said.default) {
        Some(engine) if engine == "hermes" => Gateway::Wanted,
        Some(_) => Gateway::NotWanted,
        None => Gateway::Unknown,
    }
}

/// After a round that reached the channel, with [`SWITCH`] and Hermes on,
/// after Hermes's own [`hermes_install::after_round`]: the unit written
/// (systemd reloaded when it changed); then with Hermes there
/// ([`hermes_install::present`]) and [`NS`] as this build writes it, the
/// gateway kept as `gateway` says. Hermes there but [`NS`] not: Hermes's
/// one-shot is asked to write it ([`hermes_install::ask_gateway`]) and
/// started, unless an ask or an install is already under way. Hermes
/// missing: nothing more, its own install comes first. Whether the
/// one-shot was started is returned.
pub fn after_round(
    root: &Path,
    records: &Path,
    shown: Shown,
    gateway: Gateway,
    systemd: &mut dyn Systemd,
    now_unix_ms: u64,
    say: &mut dyn FnMut(&str),
) -> bool {
    // #8320: whether a unit was there before this write without `PartOf=`
    // (an earlier build's), read before the write. A first write has no
    // gateway running yet.
    let adds_part_of = fs::read_to_string(unit_path(root))
        .is_ok_and(|text| !text.lines().any(|line| line == PART_OF));
    match crate::writer_unit::write_unit(root, UNIT, &unit_text(shown)) {
        Ok(written) => {
            let reloaded = match written.then(|| systemd.reload()) {
                Some(Err(why)) => {
                    say(&format!("{UNIT} written, but systemd not reloaded: {why}"));
                    false
                }
                _ => written,
            };
            let restarted = loaded_as_written(systemd, say);
            // #8320: a running gateway, its unit just written anew with
            // `PartOf=` over one without it (the round that ships it, which
            // restarted lamp-control before this write), is try-restarted
            // once, so its namespace binds the socket lamp-control has now.
            // Only on that write (the next round finds the same bytes);
            // `try-restart` leaves a stopped gateway stopped.
            if reloaded && !restarted && adds_part_of && shown.tasks {
                match systemd.try_restart_hermes_gateway() {
                    Ok(()) => say(&format!(
                        "{UNIT} written anew: the gateway restarted, so it shows lamp-control's socket as it is now (#8320)"
                    )),
                    Err(why) => say(&format!(
                        "{UNIT} written anew, the gateway not restarted: {why} (#8320)"
                    )),
                }
            }
        }
        Err(why) => {
            say(&format!("Hermes's gateway unit not written: {why}"));
            return false;
        }
    }
    if !hermes_install::present(root, shown) {
        return false;
    }
    if present(root, shown) {
        keep_gateway(root, systemd, gateway, true, say);
        return false;
    }
    let busy = records.join(hermes_install::ASK).exists()
        || hermes_install::installing(records)
        || openclaw_install::under_way(records, now_unix_ms);
    if busy {
        return false;
    }
    let started = hermes_install::ask_gateway(records, shown)
        .map_err(|error| error.to_string())
        .and_then(|()| crate::harness_install::start(root, systemd));
    match started {
        Ok(()) => {
            say(&format!(
                "Hermes's gateway: {} started to write {NS}",
                crate::harness_install::UNIT
            ));
            true
        }
        Err(why) => {
            say(&format!("Hermes's gateway: {NS} not asked for: {why}"));
            false
        }
    }
}

/// Between rounds, one look of [`crate::gateway_keeper`]'s: with the unit
/// and [`NS`] there, the gateway kept as `gateway` says; not wanted, it is
/// disabled and stopped whether they are there or not, so a gateway left
/// running never holds OpenClaw's start. Already as wanted, nothing is run
/// or said. A `failed` gateway (past its start limit) is left to the next
/// round, which resets it, as OpenClaw's.
pub fn keep_between_rounds(
    root: &Path,
    systemd: &mut dyn Systemd,
    gateway: Gateway,
    say: &mut dyn FnMut(&str),
) {
    let there =
        fs::symlink_metadata(on(root, NS)).is_ok() && fs::symlink_metadata(unit_path(root)).is_ok();
    if !there && gateway != Gateway::NotWanted {
        return;
    }
    let enabled = fs::symlink_metadata(on(root, WANTS)).is_ok();
    let active = gateway_state(systemd);
    let up = matches!(active.as_deref(), Some("active" | "activating"));
    let down = matches!(active.as_deref(), Some("inactive" | "failed"));
    let failed = active.as_deref() == Some("failed");
    let settled = match gateway {
        Gateway::Wanted => enabled && (up || failed),
        Gateway::NotWanted => !enabled && down,
        Gateway::Unknown => true,
    };
    if !settled {
        // Just wanted (not enabled yet: the default agent switched to
        // Hermes): a `failed` left from before is reset and started now,
        // as a round's look would, never left for a round hours away.
        let round = gateway == Gateway::Wanted && !enabled;
        keep_gateway(root, systemd, gateway, round, say);
    }
}

/// The gateway kept as `gateway` says: wanted, [`gateway_on`]; not wanted,
/// [`gateway_idle`]; unknown, nothing. `round`: a round's look, which
/// resets a `failed` gateway before its start.
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

/// Enabled at boot by [`WANTS`] (made when missing, systemd reloaded) and
/// started without waiting for `/health` when `inactive`, or on a round's
/// look when `failed`, after a `reset-failed`. Running, or a state systemd
/// doesn't say, it is left as it is.
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
                say(&format!(
                    "{UNIT} enabled: Hermes is the lamp's agent, its gateway runs"
                ));
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
                "{UNIT} failed: reset-failed, so it can be started again"
            )),
            Err(why) => say(&format!("{UNIT} failed, and not reset-failed: {why}")),
        },
        _ => return,
    }
    match systemd.start_added(UNIT) {
        Ok(()) => say(&format!("{UNIT} started: Hermes is the lamp's agent")),
        Err(why) => say(&format!("{UNIT} not started: {why}")),
    }
}

/// Not wanted: [`WANTS`] taken out (systemd reloaded), and stopped when it
/// runs. Said once, when it changes something.
fn gateway_idle(root: &Path, systemd: &mut dyn Systemd, say: &mut dyn FnMut(&str)) {
    let wants = on(root, WANTS);
    if fs::symlink_metadata(&wants).is_ok() {
        match fs::remove_file(&wants) {
            Ok(()) => {
                say(&format!(
                    "{UNIT} disabled: Hermes is not the lamp's agent, so its gateway is not started at boot"
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
            Ok(()) => say(&format!("{UNIT} stopped: Hermes is not the lamp's agent")),
            Err(why) => say(&format!("{UNIT} not stopped: {why}")),
        }
    }
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

/// The gateway's `ActiveState` as systemd says it, `None` when it can't.
fn gateway_state(systemd: &mut dyn Systemd) -> Option<String> {
    systemd
        .states(&[UNIT.to_owned()])
        .ok()
        .and_then(|states| states.into_iter().next())
        .map(|state| state.active)
}

/// After the unit is written or found as written, each round (as #7879 for
/// OpenClaw's): what systemd loaded is checked against it. A drop-in not
/// lamp-update's is said and left. A reload still owed, or a CPU cap not
/// read back as [`openclaw_install::QUOTA_READ_BACK`], reloads systemd and,
/// with the gateway running, `try-restart`s it, in one line naming the
/// check. Anything still off is found again by the next round. Whether a
/// `try-restart` was asked is returned.
fn loaded_as_written(systemd: &mut dyn Systemd, say: &mut dyn FnMut(&str)) -> bool {
    let show = match systemd.hermes_gateway_unit_properties() {
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
        (Some("no"), Some(openclaw_install::QUOTA_READ_BACK)) => return false,
        (Some("no"), quota) => format!(
            "its CPU cap reads back as {:?}, not {:?}",
            crate::world::clean_line(quota.unwrap_or("nothing")),
            openclaw_install::QUOTA_READ_BACK
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
    match systemd.try_restart_hermes_gateway() {
        Ok(()) => say(&format!(
            "{UNIT} not loaded as written ({failed}): systemd reloaded and the gateway restarted"
        )),
        Err(why) => say(&format!(
            "{UNIT} not loaded as written ({failed}): systemd reloaded, the gateway not restarted: {why}"
        )),
    }
    true
}

/// With [`SWITCH`] off, or Hermes off: a unit an earlier round wrote is
/// disabled, stopped and taken out, systemd reloaded. [`NS`] is left to
/// Hermes's own removal: nothing runs it without the unit.
pub fn after_round_off(root: &Path, systemd: &mut dyn Systemd, say: &mut dyn FnMut(&str)) {
    if fs::symlink_metadata(unit_path(root)).is_err() {
        return;
    }
    let wants = on(root, WANTS);
    if fs::symlink_metadata(&wants).is_ok()
        && let Err(error) = fs::remove_file(&wants)
    {
        say(&format!("{UNIT} not disabled: {error}"));
    }
    if let Err(why) = systemd.stop(UNIT) {
        say(&format!("{UNIT} not stopped: {why}"));
    }
    match fs::remove_file(unit_path(root)) {
        Ok(()) => {
            say(&format!("Hermes's gateway is off: {UNIT} taken out"));
            if let Err(why) = systemd.reload() {
                say(&format!("systemd not reloaded: {why}"));
            }
        }
        Err(error) => say(&format!("{UNIT} not taken out: {error}")),
    }
}

#[cfg(test)]
mod tests;
