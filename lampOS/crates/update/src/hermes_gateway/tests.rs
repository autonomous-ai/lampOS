use super::*;
use crate::judge::UnitState;

/// Every combination of the switches the namespace script follows.
fn every_shown() -> Vec<Shown> {
    let mut all = Vec::new();
    for skills in [false, true] {
        for tasks in [false, true] {
            for one_path in [false, true] {
                all.push(Shown {
                    skills,
                    tasks,
                    one_path: tasks && one_path,
                });
            }
        }
    }
    all
}

const OFF: Shown = Shown {
    skills: false,
    tasks: false,
    one_path: false,
};

/// The product's: every switch on.
const PRODUCT: Shown = Shown {
    skills: true,
    tasks: true,
    one_path: true,
};

/// What a round asked systemd, in order.
#[derive(Default)]
struct Fake {
    asked: Vec<String>,
    /// The gateway's `ActiveState`; none set: `inactive`.
    gateway: Option<&'static str>,
    /// What `systemctl show` says of the loaded unit; none: as written.
    loaded: Option<&'static str>,
}

/// The unit loaded as written, the gateway stopped.
const LOADED_AS_WRITTEN: &str = "DropInPaths=\n\
    NeedDaemonReload=no\n\
    CPUQuotaPerSecUSec=1s\n\
    ActiveState=inactive\n";

impl Systemd for Fake {
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
        assert_eq!(units, [UNIT], "only the gateway's state is asked");
        self.asked.push("gateway-state".into());
        Ok(vec![UnitState {
            unit: UNIT.to_owned(),
            active: self.gateway.unwrap_or("inactive").to_owned(),
            ..UnitState::default()
        }])
    }
    fn self_recovery_ready(&mut self) -> Result<(), String> {
        unreachable!()
    }
    fn reload(&mut self) -> Result<(), String> {
        self.asked.push("reload".into());
        Ok(())
    }
    fn restart(&mut self, _: &str) -> Result<(), String> {
        unreachable!("the gateway is started or stopped, never restarted")
    }
    fn reset_failed(&mut self, unit: &str) -> Result<(), String> {
        self.asked.push(format!("reset-failed {unit}"));
        Ok(())
    }
    fn stop(&mut self, unit: &str) -> Result<(), String> {
        self.asked.push(format!("stop {unit}"));
        Ok(())
    }
    fn start_unit(&mut self, unit: &str) -> Result<(), String> {
        unreachable!("{unit}: the gateway is started without waiting for /health")
    }
    fn start_added(&mut self, unit: &str) -> Result<(), String> {
        self.asked.push(format!("start {unit}"));
        Ok(())
    }
    fn last_line(&mut self, _: &str) -> Option<String> {
        unreachable!()
    }
    fn hermes_gateway_unit_properties(&mut self) -> Result<String, String> {
        Ok(self.loaded.unwrap_or(LOADED_AS_WRITTEN).to_owned())
    }
    fn try_restart_hermes_gateway(&mut self) -> Result<(), String> {
        self.asked.push("try-restart".into());
        Ok(())
    }
    fn harness_unit_properties(&mut self) -> Result<String, String> {
        self.asked.push("show-harness".into());
        Ok("Id=lamp-update-harness.service\n\
            LoadState=loaded\n\
            FragmentPath=/etc/systemd/system/lamp-update-harness.service\n\
            DropInPaths=\n\
            UnitFileState=static\n"
            .to_owned())
    }
    fn start_harness_unit(&mut self) -> Result<(), String> {
        self.asked.push("start-harness".into());
        Ok(())
    }
}

struct Lamp {
    root: tempfile::TempDir,
}

impl Lamp {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for folder in [
            "etc/systemd/system",
            "usr/local/bin",
            "usr/local/lib/lampos",
            "var/lib/lampos-update",
        ] {
            fs::create_dir_all(root.path().join(folder)).unwrap();
        }
        Self { root }
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
    fn records(&self) -> PathBuf {
        self.root().join("var/lib/lampos-update")
    }
    fn at(&self, path: &str) -> PathBuf {
        on(self.root(), path)
    }
    /// Hermes as this build's install leaves it for `shown`.
    fn hermes_there(&self, shown: Shown) {
        let hermes = self.at(hermes_install::HERMES);
        fs::create_dir_all(hermes.parent().unwrap()).unwrap();
        fs::write(hermes, "").unwrap();
        let git = self.at(&format!("{}/.git", hermes_install::CHECKOUT));
        fs::create_dir_all(&git).unwrap();
        fs::write(git.join("HEAD"), format!("{}\n", hermes_install::COMMIT)).unwrap();
        fs::write(
            self.at(hermes_install::WRAPPER),
            hermes_install::wrapper_for(shown),
        )
        .unwrap();
        let (ns, text) = hermes_install::ns_for(shown);
        fs::write(self.at(ns), text).unwrap();
        fs::write(
            self.at(hermes_install::RUN_ENV),
            hermes_install::RUN_ENV_TEXT,
        )
        .unwrap();
        crate::agent_ns::write_hook_shim(self.root()).unwrap();
        crate::sdk_tree::write(self.root()).unwrap();
        assert!(hermes_install::present(self.root(), shown));
    }
    /// Harness there, so its one-shot may be started.
    fn harness_there(&self) {
        let bin = self.at("/var/lib/lampos-tasks/.local/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("harness"), "").unwrap();
        std::os::unix::fs::symlink(
            "/var/lib/lampos-tasks/.local/bin/harness",
            self.at("/usr/local/bin/harness"),
        )
        .unwrap();
    }
    fn enabled(&self) -> bool {
        fs::symlink_metadata(self.at(WANTS)).is_ok()
    }
}

fn said() -> impl FnMut(&str) {
    |_: &str| {}
}

/// Each edit of the gateway's namespace script is found exactly once in
/// Hermes's script, whatever the switches, so none is silently dropped.
#[test]
fn every_namespace_edit_is_found_once_for_every_switch() {
    for shown in every_shown() {
        let mut text = hermes_install::ns_text(shown.skills, shown.tasks);
        if shown.tasks {
            assert_eq!(text.matches(hermes_install::NS_HOME_LINE).count(), 1);
            text = text.replacen(hermes_install::NS_HOME_LINE, "", 1);
            assert_eq!(text.matches(PUSHED_ONLY_IN_A_RUN).count(), 1);
            assert_eq!(text.matches("if [ -S \"$L\" ]; then").count(), 1);
        }
        for (from, to) in NS_EDITS {
            assert_eq!(text.matches(from).count(), 1, "{shown:?}: {from}");
            text = text.replacen(from, to, 1);
        }
    }
}

/// The script runs the gateway alone, as its shell's child, with the key
/// and token read before `/var/lib` is hidden, the skill-start folder shown
/// read-only, the launcher first on `PATH`, and no Hermes YOLO.
#[test]
fn the_namespace_runs_only_the_gateway_with_its_key_and_the_launcher() {
    for shown in every_shown() {
        let text = ns_text(shown);
        assert!(text.starts_with("#!/bin/sh\n# lampOS: Hermes's API gateway"));
        assert!(!text.contains(RUN_LINE), "Hermes's own run line is gone");
        assert!(text.ends_with(GATEWAY_RUN), "{shown:?}");
        assert!(!text.contains("hermes-ns: "));
        assert!(!text.contains("LAMP_NS_HOME%/home"), "no make home");
        assert!(text.contains("unset HERMES_YOLO_MODE "));
        let at = |needle: &str| {
            text.find(needle)
                .unwrap_or_else(|| panic!("{shown:?}: no {needle}"))
        };
        // Secrets read before /var/lib is hidden.
        assert!(at("cat /var/lib/lampos-hermes-gateway/api-key") < at("none /var/lib ||"));
        assert!(at("cat /var/lib/lampos-cloud/hermes-token") < at("none /var/lib ||"));
        assert!(text.contains("export LAMP_HERMES_TOKEN API_SERVER_KEY\n"));
        // The skill-start folder: staged before /run is hidden, moved back
        // and made read-only, its staging taken out.
        assert!(at("mount --bind \"$Q\" \"$S/q\"") < at("none /run ||"));
        assert!(at("mount --move \"$S/q\" \"$Q\" && ro \"$Q\"") > at("none /run ||"));
        assert!(text.contains("{ [ -z \"$Q\" ] || rmdir \"$S/q\"; } && rmdir \"$S/h\""));
        // The launcher's link on /run, before it is read-only.
        assert!(
            at("ln -s /usr/local/lib/lampos/skill-start /run/lampos-gateway-bin/skill-start")
                < at("mount -o remount,bind,ro,nosuid,nodev,noexec /run")
        );
        assert!(at("PATH=/run/lampos-gateway-bin:$PATH") > at("export PATH\n"));
        // Only `gateway run`, as a child that gets the stop's SIGTERM.
        assert!(text.contains("[ \"${1-}\" = gateway ] && [ \"${2-}\" = run ] || fail"));
        assert!(text.contains("/var/lib/lampos-hermes/.local/bin/hermes \"$@\" &\n"));
        assert!(text.contains("trap 't=TERM; kill -TERM \"$c\" 2>/dev/null' TERM\n"));
        // Ids from its own maps, not the caller's environment.
        assert!(
            at("read -r _ LAMP_NS_UID _ < /proc/self/uid_map") < at("[ -n \"${LAMP_NS_UID-}\" ]")
        );
        assert_eq!(
            text.contains("lamp-control's socket not there after 60 s"),
            shown.tasks
        );
        // The pushed skills whenever there (a run is a turn on the gateway).
        assert!(!text.contains(PUSHED_ONLY_IN_A_RUN));
        assert_eq!(text.contains(PUSHED_ALWAYS), shown.tasks);
    }
}

/// The unit: `lampos`, the namespace started by `unshare` as Hermes's
/// wrapper does, `gateway run` under an external supervisor, loopback and
/// its port, the key's folder hidden under `/var/lib`, the skill-start
/// folder kept past a stop, active only once `/health` answers.
#[test]
fn the_unit_runs_the_gateway_in_its_namespace_and_is_ready_on_health() {
    let text = unit_text(OFF);
    assert_eq!(text, UNIT_TEXT);
    for line in [
        "User=lampos\n",
        "Group=lampos\n",
        "Environment=HOME=/var/lib/lampos-hermes HERMES_HOME=/var/lib/lampos-hermes/.hermes API_SERVER_HOST=127.0.0.1 API_SERVER_PORT=8642 PATH=/usr/bin:/bin\n",
        "ExecStart=/usr/bin/unshare --map-root-user --mount --pid --fork --kill-child --mount-proc --propagation private /usr/local/lib/lampos/hermes-gateway-ns gateway run --external-supervisor\n",
        "StateDirectory=lampos-hermes-gateway\n",
        "StateDirectoryMode=0700\n",
        "RuntimeDirectory=lampos-skill-start\n",
        "RuntimeDirectoryPreserve=yes\n",
        "Restart=always\n",
        "StartLimitBurst=5\n",
        "NoNewPrivileges=yes\n",
        "WantedBy=multi-user.target\n",
        "SyslogIdentifier=lamp-hermes-gateway\n",
    ] {
        assert!(text.contains(line), "{line}");
    }
    let pre = text
        .lines()
        .find(|line| line.starts_with("ExecStartPre="))
        .unwrap();
    assert!(pre.contains("K=/var/lib/lampos-hermes-gateway/api-key"));
    assert!(pre.contains("umask 077"));
    assert!(pre.contains("od -An -N32 -tx1 /dev/urandom | tr -dc 0-9a-f"));
    let post = text
        .lines()
        .find(|line| line.starts_with("ExecStartPost="))
        .unwrap();
    assert!(post.contains(HEALTH_URL));
    // Every shell variable escaped for systemd ($$), no specifier (%).
    for line in [pre, post] {
        assert!(!line.contains('%'), "{line}");
        assert!(
            line.match_indices('$')
                .all(|(at, _)| line[at..].starts_with("$$") || line[..at].ends_with('$')),
            "{line}"
        );
    }
    assert!(text.contains(&format!("{HOST}:{PORT}")));
    // The lamp's network, as OpenClaw's gateway: no network namespace.
    let start = text
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .unwrap();
    assert!(!start.contains("--net"), "{start}");
    assert!(KEY.starts_with(KEY_DIR));
    // With the tasks switch on, after lamp-control.
    let tasks = unit_text(PRODUCT);
    assert!(tasks.contains(
        "After=network-online.target lamp-cloud.service lamp-control.service\nWants=lamp-control.service\n"
    ));
}

/// The rule: only Hermes as the default wants it; another default does
/// not; no answer, or no default, leaves it.
#[test]
fn the_gateway_is_wanted_only_while_hermes_is_the_default() {
    let said = |default: Option<&str>| {
        Some(Said {
            default: default.map(str::to_owned),
            openclaw_in_use: Some(false),
        })
    };
    assert_eq!(gateway_wanted(said(Some("hermes"))), Gateway::Wanted);
    assert_eq!(gateway_wanted(said(Some("openclaw"))), Gateway::NotWanted);
    assert_eq!(gateway_wanted(said(None)), Gateway::Unknown);
    assert_eq!(gateway_wanted(None), Gateway::Unknown);
}

/// A round with Hermes missing writes the unit and nothing more.
#[test]
fn a_round_without_hermes_writes_only_the_unit() {
    let lamp = Lamp::new();
    let mut systemd = Fake::default();
    let asked = after_round(
        lamp.root(),
        &lamp.records(),
        OFF,
        Gateway::Wanted,
        &mut systemd,
        1,
        &mut said(),
    );
    assert!(!asked);
    assert_eq!(
        fs::read_to_string(unit_path(lamp.root())).unwrap(),
        UNIT_TEXT
    );
    assert_eq!(systemd.asked, ["reload"]);
    assert!(!lamp.enabled());
    assert!(!lamp.records().join(hermes_install::ASK).exists());
}

/// Hermes there, its gateway's script not: the round asks Hermes's one-shot
/// with the gateway and starts it, once; the one-shot's answer written, the
/// next round enables and starts the gateway without waiting for /health.
#[test]
fn a_round_asks_the_one_shot_for_the_script_then_starts_the_gateway() {
    let lamp = Lamp::new();
    lamp.hermes_there(PRODUCT);
    lamp.harness_there();
    let mut systemd = Fake::default();
    let round = |systemd: &mut Fake| {
        after_round(
            lamp.root(),
            &lamp.records(),
            PRODUCT,
            Gateway::Wanted,
            systemd,
            1,
            &mut said(),
        )
    };
    assert!(round(&mut systemd));
    assert_eq!(
        systemd.asked,
        ["reload", "reload", "show-harness", "start-harness"],
        "the gateway's unit and Harness's one-shot's written, the one-shot started"
    );
    let ask = fs::read_to_string(lamp.records().join(hermes_install::ASK)).unwrap();
    assert!(ask.contains("\"gateway\":true"), "{ask}");
    assert!(!lamp.enabled(), "nothing to run yet");
    // The ask not answered yet: never asked twice.
    systemd.asked.clear();
    assert!(!round(&mut systemd));
    assert!(systemd.asked.is_empty(), "{:?}", systemd.asked);
    // The one-shot wrote the script and spent the ask.
    write_ns(lamp.root(), PRODUCT).unwrap();
    fs::remove_file(lamp.records().join(hermes_install::ASK)).unwrap();
    assert!(present(lamp.root(), PRODUCT));
    assert!(!round(&mut systemd));
    assert!(lamp.enabled());
    assert_eq!(
        fs::read_link(lamp.at(WANTS)).unwrap(),
        Path::new("/etc/systemd/system/lamp-hermes-gateway.service")
    );
    assert_eq!(
        systemd.asked,
        ["reload", "gateway-state", format!("start {UNIT}").as_str()]
    );
    // Running: nothing more.
    systemd.asked.clear();
    systemd.gateway = Some("active");
    assert!(!round(&mut systemd));
    assert_eq!(systemd.asked, ["gateway-state"]);
    // Failed past its start limit: a round resets it and starts it.
    systemd.asked.clear();
    systemd.gateway = Some("failed");
    round(&mut systemd);
    assert_eq!(
        systemd.asked,
        [
            "gateway-state".to_owned(),
            format!("reset-failed {UNIT}"),
            format!("start {UNIT}")
        ]
    );
}

/// Another default: disabled and stopped, by a round or between rounds;
/// unknown leaves it; Hermes the default again starts it between rounds.
#[test]
fn another_default_stops_the_gateway_and_hermes_again_starts_it() {
    let lamp = Lamp::new();
    lamp.hermes_there(OFF);
    write_ns(lamp.root(), OFF).unwrap();
    let mut systemd = Fake {
        gateway: Some("active"),
        ..Fake::default()
    };
    after_round(
        lamp.root(),
        &lamp.records(),
        OFF,
        Gateway::Wanted,
        &mut systemd,
        1,
        &mut said(),
    );
    assert!(lamp.enabled());
    systemd.asked.clear();
    after_round(
        lamp.root(),
        &lamp.records(),
        OFF,
        Gateway::NotWanted,
        &mut systemd,
        2,
        &mut said(),
    );
    assert!(!lamp.enabled());
    assert_eq!(
        systemd.asked,
        [
            "reload".to_owned(),
            "gateway-state".to_owned(),
            format!("stop {UNIT}")
        ]
    );
    // Between rounds, stopped and disabled already: nothing run.
    systemd.gateway = Some("inactive");
    systemd.asked.clear();
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::NotWanted, &mut said());
    assert_eq!(systemd.asked, ["gateway-state"]);
    // Unknown: left as it is.
    systemd.asked.clear();
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::Unknown, &mut said());
    assert_eq!(systemd.asked, ["gateway-state"]);
    assert!(!lamp.enabled());
    // Hermes chosen again: enabled and started within one look.
    systemd.asked.clear();
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::Wanted, &mut said());
    assert!(lamp.enabled());
    assert_eq!(
        systemd.asked,
        [
            "gateway-state".to_owned(),
            "reload".to_owned(),
            "gateway-state".to_owned(),
            format!("start {UNIT}")
        ]
    );
    // A failed gateway is left to the next round between rounds.
    systemd.gateway = Some("failed");
    systemd.asked.clear();
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::Wanted, &mut said());
    assert_eq!(systemd.asked, ["gateway-state"]);
}

/// Between rounds, nothing is touched before the unit and the script are
/// both there.
#[test]
fn between_rounds_nothing_runs_without_the_unit_and_the_script() {
    let lamp = Lamp::new();
    let mut systemd = Fake::default();
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::Wanted, &mut said());
    write_ns(lamp.root(), OFF).unwrap();
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::Wanted, &mut said());
    assert!(systemd.asked.is_empty(), "{:?}", systemd.asked);
    assert!(!lamp.enabled());
}

/// The switch off: the unit disabled, stopped and taken out, systemd
/// reloaded; with no unit, nothing is asked.
#[test]
fn the_switch_off_takes_the_unit_out() {
    let lamp = Lamp::new();
    let mut systemd = Fake::default();
    after_round_off(lamp.root(), &mut systemd, &mut said());
    assert!(systemd.asked.is_empty());
    lamp.hermes_there(OFF);
    write_ns(lamp.root(), OFF).unwrap();
    after_round(
        lamp.root(),
        &lamp.records(),
        OFF,
        Gateway::Wanted,
        &mut systemd,
        1,
        &mut said(),
    );
    assert!(lamp.enabled());
    systemd.asked.clear();
    after_round_off(lamp.root(), &mut systemd, &mut said());
    assert!(!lamp.enabled());
    assert!(!unit_path(lamp.root()).exists());
    assert_eq!(systemd.asked, [format!("stop {UNIT}"), "reload".to_owned()]);
}

/// The script written by the one-shot: root's 0755 file, this build's
/// text; a link in its place is never taken for it.
#[test]
fn the_script_is_present_only_as_this_builds_file() {
    use std::os::unix::fs::PermissionsExt;
    let lamp = Lamp::new();
    assert!(!present(lamp.root(), OFF));
    write_ns(lamp.root(), OFF).unwrap();
    assert!(present(lamp.root(), OFF));
    assert!(
        !present(lamp.root(), PRODUCT),
        "another switch, another text"
    );
    let mode = fs::metadata(lamp.at(NS)).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode, 0o755);
    let elsewhere = lamp.root().join("elsewhere");
    fs::write(&elsewhere, ns_text(OFF)).unwrap();
    fs::remove_file(lamp.at(NS)).unwrap();
    std::os::unix::fs::symlink(&elsewhere, lamp.at(NS)).unwrap();
    assert!(!present(lamp.root(), OFF));
    let mut lines = Vec::new();
    remove_ns(lamp.root(), &mut |line| lines.push(line.to_owned()));
    assert!(!lamp.at(NS).exists() && elsewhere.exists());
    assert_eq!(lines, [format!("{NS} taken out")]);
}

/// As #7879 for OpenClaw's: a reload still owed, or the CPU cap not read
/// back, reloads systemd and restarts a running gateway; a drop-in is only
/// said; as written, nothing is run.
#[test]
fn a_unit_not_loaded_as_written_is_reloaded_and_restarted() {
    let lamp = Lamp::new();
    let round = |loaded: &'static str| {
        let mut systemd = Fake {
            loaded: Some(loaded),
            ..Fake::default()
        };
        let mut lines = Vec::new();
        after_round(
            lamp.root(),
            &lamp.records(),
            OFF,
            Gateway::Wanted,
            &mut systemd,
            1,
            &mut |line| lines.push(line.to_owned()),
        );
        (systemd.asked, lines)
    };
    // The first round writes the unit: one reload for that.
    let (asked, lines) = round(LOADED_AS_WRITTEN);
    assert_eq!(asked, ["reload"]);
    assert!(lines.is_empty(), "{lines:?}");
    let (asked, lines) =
        round("DropInPaths=\nNeedDaemonReload=yes\nCPUQuotaPerSecUSec=1s\nActiveState=active\n");
    assert_eq!(asked, ["reload", "try-restart"]);
    assert!(lines[0].contains("NeedDaemonReload"), "{lines:?}");
    let (asked, _) = round(
        "DropInPaths=\nNeedDaemonReload=no\nCPUQuotaPerSecUSec=infinity\nActiveState=inactive\n",
    );
    assert_eq!(asked, ["reload"]);
    let (asked, lines) = round(
        "DropInPaths=/etc/systemd/system/lamp-hermes-gateway.service.d/x.conf\nNeedDaemonReload=yes\n",
    );
    assert!(asked.is_empty(), "{asked:?}");
    assert!(lines[0].contains("drop-in"), "{lines:?}");
}

/// A direct make writes in lamp-tasks' work folder: the gateway sees it
/// read-write, whole, as OpenClaw's gateway does (only a skill's run of its
/// own, LAMP_NS_NET=run, is narrowed to its folder, and the gateway never
/// sets it).
#[test]
fn the_work_folder_is_shown_read_write_for_makes() {
    for shown in every_shown() {
        let text = ns_text(shown);
        assert!(text.contains("W=/var/lib/lampos-tasks/harnesses/lamp-tasks "));
        assert!(
            text.contains("mount --bind \"$W\" \"$S/w\"")
                || text.contains("mount --bind \"${Z:-$W}\" \"$S/w\""),
            "{shown:?}"
        );
        assert!(!text.contains("ro \"$W\""), "{shown:?}");
        assert!(!text.contains("$W/lamp not hidden"), "{shown:?}");
    }
}
