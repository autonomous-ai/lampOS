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
    // #8320: with the tasks switch on, restarted with lamp-control (its
    // namespace binds lamp-control's socket file once, at its start); off,
    // never tied to it.
    for shown in every_shown() {
        let unit = unit_text(shown);
        let part_of = unit.lines().filter(|line| *line == PART_OF).count();
        assert_eq!(part_of, usize::from(shown.tasks), "{shown:?}\n{unit}");
        assert_eq!(unit.matches("PartOf=").count(), part_of, "{unit}");
    }
}

/// #8320 (the second read's P2 on #8431): the round that ships `PartOf=`
/// writes it over an earlier build's unit without it, after a round's
/// switch restarted lamp-control; the running gateway still binds the old
/// socket, so it is try-restarted once. The next round finds the same
/// bytes and restarts nothing; a first write (no gateway yet) and the tasks
/// switch off never restart it.
#[test]
fn the_round_that_adds_part_of_restarts_the_running_gateway_once() {
    let lamp = Lamp::new();
    let round = |shown: Shown| {
        let mut systemd = Fake {
            gateway: Some("active"),
            ..Fake::default()
        };
        let mut lines = Vec::new();
        after_round(
            lamp.root(),
            &lamp.records(),
            shown,
            Gateway::Wanted,
            &mut systemd,
            1,
            &mut |line| lines.push(line.to_owned()),
        );
        (systemd.asked, lines)
    };
    // A first write: no unit before, nothing to restart.
    let (asked, _) = round(PRODUCT);
    assert_eq!(asked, ["reload"]);
    // An earlier build's unit, without `PartOf=`.
    let before = unit_text(PRODUCT).replacen("PartOf=lamp-control.service\n", "", 1);
    fs::write(unit_path(lamp.root()), &before).unwrap();
    let (asked, lines) = round(PRODUCT);
    assert_eq!(asked, ["reload", "try-restart"]);
    assert!(lines.iter().any(|line| line.contains("#8320")), "{lines:?}");
    // The same bytes next round: nothing.
    let (asked, _) = round(PRODUCT);
    assert!(asked.is_empty(), "{asked:?}");
    // The tasks switch off: written without `PartOf=`, never restarted.
    fs::write(unit_path(lamp.root()), &before).unwrap();
    let (asked, _) = round(OFF);
    assert_eq!(asked, ["reload"]);
}

/// The rule: only Hermes as the default wants it; another default does
/// not; no answer, or no default, leaves it.
#[test]
fn the_gateway_is_wanted_only_while_hermes_is_the_default() {
    let said = |default: Option<&str>| {
        Some(Said {
            default: default.map(str::to_owned),
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
    // A failed gateway, already wanted, is left to the next round.
    systemd.gateway = Some("failed");
    systemd.asked.clear();
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::Wanted, &mut said());
    assert_eq!(systemd.asked, ["gateway-state"]);
    // Not wanted again, then Hermes chosen once more with the gateway left
    // failed by the stop before (lamp-ee17, 24 Nov): reset and started at
    // once, not left for a round hours away.
    systemd.gateway = Some("inactive");
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::NotWanted, &mut said());
    assert!(!lamp.enabled());
    systemd.gateway = Some("failed");
    systemd.asked.clear();
    keep_between_rounds(lamp.root(), &mut systemd, Gateway::Wanted, &mut said());
    assert!(lamp.enabled());
    assert_eq!(
        systemd.asked,
        [
            "gateway-state".to_owned(),
            "reload".to_owned(),
            "gateway-state".to_owned(),
            format!("reset-failed {UNIT}"),
            format!("start {UNIT}")
        ]
    );
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

/// The two agent gateways side by side, driven as lamp-update drives them
/// (looks of its one keeper, crate::gateway_keeper, and a round), on one
/// fake systemd that keeps both units' states and says whether both ever
/// ran at once.
mod one_gateway {
    use super::*;
    use crate::gateway_keeper::{Keeper, Kept, round_gateways};
    use crate::openclaw_install as oc;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Two {
        active: BTreeMap<String, &'static str>,
        log: Vec<String>,
        /// A unit whose stop fails, left running.
        stop_fails: Option<&'static str>,
        /// Both gateways running at once, after any call.
        ever_both: bool,
    }

    impl Two {
        fn is(&self, unit: &str) -> &'static str {
            self.active.get(unit).copied().unwrap_or("inactive")
        }
        fn both_running(&self) -> bool {
            let up = |u: &str| matches!(self.is(u), "active" | "activating" | "deactivating");
            up(UNIT) && up(oc::UNIT)
        }
        fn up(&mut self, unit: &str) {
            self.active.insert(unit.to_owned(), "active");
            self.ever_both |= self.both_running();
        }
        fn at(&self, line: &str) -> usize {
            self.log
                .iter()
                .position(|l| l == line)
                .unwrap_or_else(|| panic!("no `{line}` in {:?}", self.log))
        }
    }

    impl Systemd for Two {
        fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
            Ok(units
                .iter()
                .map(|u| UnitState {
                    unit: u.clone(),
                    active: self.is(u).to_owned(),
                    ..UnitState::default()
                })
                .collect())
        }
        fn self_recovery_ready(&mut self) -> Result<(), String> {
            unreachable!()
        }
        fn reload(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn restart(&mut self, u: &str) -> Result<(), String> {
            unreachable!("{u}")
        }
        fn reset_failed(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn stop(&mut self, u: &str) -> Result<(), String> {
            self.log.push(format!("stop {u}"));
            if self.stop_fails == Some(u) {
                return Err(format!("{u}: Job for {u} canceled"));
            }
            self.active.insert(u.to_owned(), "inactive");
            Ok(())
        }
        fn start_unit(&mut self, u: &str) -> Result<(), String> {
            self.log.push(format!("start {u}"));
            self.up(u);
            Ok(())
        }
        fn start_added(&mut self, u: &str) -> Result<(), String> {
            self.start_unit(u)
        }
        fn last_line(&mut self, _: &str) -> Option<String> {
            None
        }
        fn hermes_gateway_unit_properties(&mut self) -> Result<String, String> {
            Ok(LOADED_AS_WRITTEN.to_owned())
        }
        fn try_restart_hermes_gateway(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn harness_unit_properties(&mut self) -> Result<String, String> {
            Ok("Id=lamp-update-harness.service\nLoadState=loaded\n\
                FragmentPath=/etc/systemd/system/lamp-update-harness.service\n\
                DropInPaths=\nUnitFileState=static\n"
                .to_owned())
        }
        fn start_harness_unit(&mut self) -> Result<(), String> {
            self.log.push("start-harness".into());
            Ok(())
        }
    }

    fn said(default: Option<&str>) -> Option<Said> {
        Some(Said {
            default: default.map(str::to_owned),
        })
    }

    /// OpenClaw there as a stable lamp has it: wrapper, unit, enabled.
    fn openclaw_there(lamp: &Lamp) {
        fs::write(lamp.at(oc::WRAPPER), "").unwrap();
        fs::write(
            lamp.root().join(crate::writer_unit::FOLDER).join(oc::UNIT),
            oc::unit_text_for(PRODUCT),
        )
        .unwrap();
        let wants = lamp.at(oc::WANTS);
        fs::create_dir_all(wants.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(format!("/etc/systemd/system/{}", oc::UNIT), wants).unwrap();
    }

    /// Both switches on, as the product has them; `one_path` OpenClaw's.
    fn keeper(one_path: bool) -> Keeper {
        Keeper::new(Kept {
            openclaw: Some(one_path),
            hermes: true,
        })
    }

    /// One keeper look, every command answered, nothing said.
    fn look(keeper: &mut Keeper, lamp: &Lamp, systemd: &mut Two, s: Option<Said>) {
        let mut run = |_: &[String]| Ok::<String, String>(String::new());
        keeper.look(lamp.root(), systemd, s, &mut run, &mut |_| {});
    }

    fn lamp_with_both(openclaw_up: bool, hermes_up: bool) -> (Lamp, Two) {
        let lamp = Lamp::new();
        lamp.hermes_there(PRODUCT);
        write_ns(lamp.root(), PRODUCT).unwrap();
        crate::writer_unit::write_unit(lamp.root(), UNIT, &unit_text(PRODUCT)).unwrap();
        openclaw_there(&lamp);
        let mut systemd = Two::default();
        if openclaw_up {
            systemd.active.insert(oc::UNIT.into(), "active");
        }
        if hermes_up {
            systemd.active.insert(UNIT.into(), "active");
            let wants = lamp.at(WANTS);
            std::os::unix::fs::symlink(format!("/etc/systemd/system/{UNIT}"), wants).unwrap();
        }
        if !openclaw_up {
            fs::remove_file(lamp.at(oc::WANTS)).unwrap();
        }
        (lamp, systemd)
    }

    /// The rule itself: whatever lamp-tasks says, the two never both want.
    #[test]
    fn one_answer_never_wants_both_gateways() {
        let mut both = Vec::new();
        for default in [Some("hermes"), Some("openclaw"), Some("claude"), None] {
            let s = said(default);
            if gateway_wanted(s.clone()) == Gateway::Wanted
                && oc::gateway_wanted(true, s) == Gateway::Wanted
            {
                both.push(format!("{default:?}"));
            }
        }
        assert!(both.is_empty(), "both gateways wanted for {both:?}");
    }

    /// (a) OpenClaw -> Hermes right after an OpenClaw turn (#8105's 4 h hold
    /// is gone): one look leaves Hermes's gateway alone, never both.
    #[test]
    fn a_switch_to_hermes_after_an_openclaw_turn_leaves_one_gateway() {
        let (lamp, mut systemd) = lamp_with_both(true, false);
        look(
            &mut keeper(false),
            &lamp,
            &mut systemd,
            said(Some("hermes")),
        );
        assert!(!systemd.ever_both, "{:?}", systemd.log);
        assert_eq!(systemd.is(UNIT), "active");
        assert_eq!(systemd.is(oc::UNIT), "inactive");
    }

    /// (a) both ways: one look each, one gateway at every step (the two
    /// threads before had both up between their looks).
    #[test]
    fn a_switch_either_way_leaves_one_gateway_after_one_look_each() {
        let (lamp, mut systemd) = lamp_with_both(true, false);
        let mut keeper = keeper(false);
        look(&mut keeper, &lamp, &mut systemd, said(Some("hermes")));
        assert_eq!(systemd.is(UNIT), "active");
        assert_eq!(systemd.is(oc::UNIT), "inactive");
        assert!(lamp.enabled() && fs::symlink_metadata(lamp.at(oc::WANTS)).is_err());
        look(&mut keeper, &lamp, &mut systemd, said(Some("openclaw")));
        assert_eq!(systemd.is(oc::UNIT), "active");
        assert_eq!(systemd.is(UNIT), "inactive");
        assert!(!lamp.enabled() && fs::symlink_metadata(lamp.at(oc::WANTS)).is_ok());
        assert!(!systemd.ever_both, "{:?}", systemd.log);
    }

    /// (b) lamp-tasks restarting (no answer) changes nothing either way.
    #[test]
    fn lamp_tasks_down_touches_neither_gateway() {
        let (lamp, mut systemd) = lamp_with_both(false, true);
        let mut keeper = keeper(false);
        look(&mut keeper, &lamp, &mut systemd, None);
        look(&mut keeper, &lamp, &mut systemd, said(None));
        assert!(systemd.log.is_empty(), "{:?}", systemd.log);
    }

    /// (c) stable -> this build, Hermes the default: the stable lamp's
    /// always-on OpenClaw gateway is stopped and disabled by the first
    /// look, Hermes's started only once the round wrote its unit and the
    /// one-shot its script; never both at any step.
    #[test]
    fn an_update_on_a_hermes_lamp_never_runs_both() {
        let lamp = Lamp::new();
        lamp.hermes_there(PRODUCT);
        lamp.harness_there();
        openclaw_there(&lamp);
        let mut systemd = Two::default();
        systemd.active.insert(oc::UNIT.into(), "active");
        let s = said(Some("hermes"));
        let mut keeper = keeper(false);
        // New lamp-update's keeper, before its first round.
        look(&mut keeper, &lamp, &mut systemd, s.clone());
        assert_eq!(systemd.is(oc::UNIT), "inactive");
        // Its round: the unit written, the one-shot asked for the script.
        let asked = after_round(
            lamp.root(),
            &lamp.records(),
            PRODUCT,
            gateway_wanted(s.clone()),
            &mut systemd,
            1,
            &mut |_| {},
        );
        assert!(asked);
        // The one-shot wrote the script; the next look starts Hermes's.
        write_ns(lamp.root(), PRODUCT).unwrap();
        look(&mut keeper, &lamp, &mut systemd, s);
        assert_eq!(systemd.is(UNIT), "active");
        assert!(!systemd.ever_both, "{:?}", systemd.log);
    }

    /// (c) stable -> this build, OpenClaw the default: Hermes's unit is
    /// written but never enabled or started; OpenClaw's left running.
    #[test]
    fn an_update_on_an_openclaw_lamp_never_starts_hermes() {
        let lamp = Lamp::new();
        lamp.hermes_there(PRODUCT);
        lamp.harness_there();
        openclaw_there(&lamp);
        let mut systemd = Two::default();
        systemd.active.insert(oc::UNIT.into(), "active");
        let s = said(Some("openclaw"));
        let mut keeper = keeper(false);
        look(&mut keeper, &lamp, &mut systemd, s.clone());
        after_round(
            lamp.root(),
            &lamp.records(),
            PRODUCT,
            gateway_wanted(s.clone()),
            &mut systemd,
            1,
            &mut |_| {},
        );
        write_ns(lamp.root(), PRODUCT).unwrap();
        look(&mut keeper, &lamp, &mut systemd, s.clone());
        after_round(
            lamp.root(),
            &lamp.records(),
            PRODUCT,
            gateway_wanted(s),
            &mut systemd,
            2,
            &mut |_| {},
        );
        assert_eq!(systemd.is(UNIT), "inactive");
        assert!(!lamp.enabled());
        assert_eq!(systemd.is(oc::UNIT), "active");
        assert!(
            systemd.log.iter().all(|l| !l.contains(oc::UNIT)),
            "{:?}",
            systemd.log
        );
    }

    /// One look, Hermes now the default with OpenClaw's gateway up:
    /// OpenClaw's is stopped before Hermes's is started. A stop that fails
    /// (OpenClaw's still running) holds Hermes's start, look after look,
    /// until OpenClaw's is down.
    #[test]
    fn a_look_stops_openclaw_before_it_starts_hermes() {
        let (lamp, mut systemd) = lamp_with_both(true, false);
        look(
            &mut keeper(false),
            &lamp,
            &mut systemd,
            said(Some("hermes")),
        );
        assert!(
            systemd.at(&format!("stop {}", oc::UNIT)) < systemd.at(&format!("start {UNIT}")),
            "{:?}",
            systemd.log
        );
        assert!(!systemd.ever_both);

        let (lamp, mut systemd) = lamp_with_both(true, false);
        systemd.stop_fails = Some(oc::UNIT);
        let mut keeper = keeper(false);
        for _ in 0..2 {
            look(&mut keeper, &lamp, &mut systemd, said(Some("hermes")));
            assert_eq!(systemd.is(UNIT), "inactive", "{:?}", systemd.log);
            assert!(!lamp.enabled());
        }
        systemd.stop_fails = None;
        look(&mut keeper, &lamp, &mut systemd, said(Some("hermes")));
        assert_eq!(systemd.is(UNIT), "active");
        assert_eq!(systemd.is(oc::UNIT), "inactive");
        assert!(!systemd.ever_both, "{:?}", systemd.log);
    }

    /// The reverse: OpenClaw now the default with Hermes's gateway up:
    /// Hermes's is stopped first, then OpenClaw's started; a failed stop
    /// holds OpenClaw's.
    #[test]
    fn a_look_stops_hermes_before_it_starts_openclaw() {
        let (lamp, mut systemd) = lamp_with_both(false, true);
        look(
            &mut keeper(false),
            &lamp,
            &mut systemd,
            said(Some("openclaw")),
        );
        assert!(
            systemd.at(&format!("stop {UNIT}")) < systemd.at(&format!("start {}", oc::UNIT)),
            "{:?}",
            systemd.log
        );
        assert!(!systemd.ever_both);

        let (lamp, mut systemd) = lamp_with_both(false, true);
        systemd.stop_fails = Some(UNIT);
        look(
            &mut keeper(false),
            &lamp,
            &mut systemd,
            said(Some("openclaw")),
        );
        assert_eq!(systemd.is(oc::UNIT), "inactive", "{:?}", systemd.log);
        assert!(!systemd.ever_both);
    }

    /// A cold OpenClaw gateway started by a look, the one run path on: its
    /// allow-list add answers "not ready". The look runs the add once and
    /// returns (no 40 × 3 s wait under the lock); the entry stays owed and
    /// is tried once at each next look until taken. A switch to Hermes
    /// meanwhile drops it and never leaves both up.
    #[test]
    fn a_cold_openclaw_gateway_never_holds_a_look() {
        use std::cell::Cell;
        let (lamp, mut systemd) = lamp_with_both(false, false);
        let adds = Cell::new(0);
        let ready = Cell::new(false);
        let mut run = |argv: &[String]| {
            let line = argv.join(" ");
            if line.contains(" approvals allowlist add ") {
                adds.set(adds.get() + 1);
                if !ready.get() {
                    return Err(format!("openclaw: {}", oc::CONFIG_NOT_READY));
                }
            }
            Ok(String::new())
        };
        let mut keeper = keeper(true);
        let mut lines = Vec::new();
        for n in 1..=2 {
            let began = std::time::Instant::now();
            keeper.look(
                lamp.root(),
                &mut systemd,
                said(Some("openclaw")),
                &mut run,
                &mut |line: &str| lines.push(line.to_owned()),
            );
            assert!(began.elapsed() < std::time::Duration::from_secs(1));
            assert_eq!(adds.get(), n, "one add per look");
            assert!(keeper.entry_owed());
        }
        assert_eq!(systemd.is(oc::UNIT), "active");
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.contains("waits for OpenClaw's gateway"))
                .count(),
            1,
            "{lines:?}"
        );
        // Still cold, the default moves to Hermes: no add, one gateway.
        keeper.look(
            lamp.root(),
            &mut systemd,
            said(Some("hermes")),
            &mut run,
            &mut |_| {},
        );
        assert_eq!(adds.get(), 2);
        assert!(!keeper.entry_owed());
        assert_eq!(systemd.is(oc::UNIT), "inactive");
        assert_eq!(systemd.is(UNIT), "active");
        // Back to OpenClaw, ready at once: the add is taken in that look.
        ready.set(true);
        keeper.look(
            lamp.root(),
            &mut systemd,
            said(Some("openclaw")),
            &mut run,
            &mut |_| {},
        );
        assert_eq!(adds.get(), 3);
        assert!(!keeper.entry_owed());
        // Owed again and ready at the next look: taken there, once.
        ready.set(false);
        keeper.look(
            lamp.root(),
            &mut systemd,
            said(Some("hermes")),
            &mut run,
            &mut |_| {},
        );
        keeper.look(
            lamp.root(),
            &mut systemd,
            said(Some("openclaw")),
            &mut run,
            &mut |_| {},
        );
        assert!(keeper.entry_owed());
        ready.set(true);
        keeper.look(
            lamp.root(),
            &mut systemd,
            said(Some("openclaw")),
            &mut run,
            &mut |_| {},
        );
        assert!(!keeper.entry_owed());
        keeper.look(
            lamp.root(),
            &mut systemd,
            said(Some("openclaw")),
            &mut run,
            &mut |_| {},
        );
        assert_eq!(adds.get(), 5, "settled: nothing more run");
        assert!(!systemd.ever_both, "{:?}", systemd.log);
    }

    /// Harness's one-shot at work, OpenClaw not wanted: the keeper still
    /// stops and disables OpenClaw's gateway (it used to leave it to the
    /// one-shot for as long as it ran), then starts Hermes's.
    #[test]
    fn the_one_shot_at_work_never_keeps_an_unwanted_openclaw_up() {
        let (lamp, mut systemd) = lamp_with_both(true, false);
        systemd
            .active
            .insert(crate::harness_install::UNIT.into(), "active");
        look(
            &mut keeper(false),
            &lamp,
            &mut systemd,
            said(Some("hermes")),
        );
        assert_eq!(systemd.is(oc::UNIT), "inactive", "{:?}", systemd.log);
        assert!(fs::symlink_metadata(lamp.at(oc::WANTS)).is_err());
        assert_eq!(systemd.is(UNIT), "active");
        assert!(!systemd.ever_both);
    }

    /// A round as main runs it, Hermes the default with OpenClaw's gateway
    /// up: `round_gateways` first, then Hermes's step, then OpenClaw's.
    /// OpenClaw's is stopped before Hermes's is started; with that stop
    /// failing, the round never starts Hermes's.
    #[test]
    fn a_round_stops_openclaw_before_it_starts_hermes() {
        for stop_fails in [false, true] {
            let (lamp, mut systemd) = lamp_with_both(true, false);
            lamp.harness_there();
            if stop_fails {
                systemd.stop_fails = Some(oc::UNIT);
            }
            let s = said(Some("hermes"));
            let (openclaw, hermes) = round_gateways(
                lamp.root(),
                &mut systemd,
                Some(oc::gateway_wanted(true, s.clone())),
                Some(gateway_wanted(s)),
                &mut |_| {},
            );
            after_round(
                lamp.root(),
                &lamp.records(),
                PRODUCT,
                hermes.unwrap(),
                &mut systemd,
                1,
                &mut |_| {},
            );
            oc::after_round_with(
                lamp.root(),
                &lamp.records(),
                PRODUCT,
                openclaw.unwrap(),
                true,
                &mut systemd,
                &mut |_: &[String]| Ok(String::new()),
                1,
                Ok(crate::hermes_schedule::Window::Reached),
                &mut |_| {},
            );
            assert!(
                !systemd.ever_both,
                "stop_fails={stop_fails}: {:?}",
                systemd.log
            );
            if stop_fails {
                assert_eq!(systemd.is(UNIT), "inactive", "{:?}", systemd.log);
            } else {
                assert!(
                    systemd.at(&format!("stop {}", oc::UNIT))
                        < systemd.at(&format!("start {UNIT}")),
                    "{:?}",
                    systemd.log
                );
            }
        }
    }
}
