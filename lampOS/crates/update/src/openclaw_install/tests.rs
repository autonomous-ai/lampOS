use super::*;
use crate::{hermes_install::Carried, judge::UnitState};
use std::{cell::RefCell, os::unix::fs::symlink, rc::Rc};

/// What the namespace shows with only the skills switch read.
fn sk(skills: bool) -> Shown {
    Shown {
        skills,
        ..Shown::default()
    }
}

/// What a round asked systemd, in order.
#[derive(Default)]
struct Fake {
    asked: Vec<&'static str>,
    /// Harness's one-shot's `ActiveState`, for the watch's acts.
    active: Option<&'static str>,
    /// What `systemctl show` says of the gateway's loaded unit (#7879):
    /// none, the unit as written, with the gateway stopped.
    loaded: Option<&'static str>,
    /// How many of the next reloads fail.
    reloads_fail: usize,
    /// How many times the gateway's loaded unit was shown.
    shown: usize,
    /// The gateway's own `ActiveState` (#8105); none set: `inactive`, as a
    /// lamp's first round finds it.
    gateway: Option<&'static str>,
}

/// The gateway's unit loaded as written, the gateway stopped.
const LOADED_AS_WRITTEN: &str = "DropInPaths=\n\
    NeedDaemonReload=no\n\
    CPUQuotaPerSecUSec=1s\n\
    ActiveState=inactive\n";

impl Systemd for Fake {
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
        if units == [UNIT] {
            let active = self.gateway.unwrap_or("inactive");
            self.asked.push("gateway-state");
            return Ok(vec![UnitState {
                unit: UNIT.to_owned(),
                active: active.to_owned(),
                ..UnitState::default()
            }]);
        }
        let active = self.active.expect("only the watch asks a unit's state");
        assert_eq!(units, [harness_install::UNIT]);
        self.asked.push("state");
        Ok(vec![UnitState {
            unit: harness_install::UNIT.to_owned(),
            active: active.to_owned(),
            ..UnitState::default()
        }])
    }
    fn freeze_harness_unit(&mut self) -> Result<(), String> {
        self.asked.push("freeze");
        Ok(())
    }
    fn thaw_harness_unit(&mut self) -> Result<(), String> {
        self.asked.push("thaw");
        Ok(())
    }
    fn stop(&mut self, unit: &str) -> Result<(), String> {
        if unit == UNIT {
            self.asked.push("stop-gateway");
        } else {
            assert_eq!(unit, harness_install::UNIT);
            self.asked.push("stop");
        }
        Ok(())
    }
    fn reset_failed(&mut self, unit: &str) -> Result<(), String> {
        assert_eq!(unit, UNIT, "a round resets only the gateway");
        self.asked.push("reset-failed");
        Ok(())
    }
    fn start_unit(&mut self, unit: &str) -> Result<(), String> {
        assert_eq!(unit, UNIT, "a round starts only the gateway");
        self.asked.push("start-gateway");
        Ok(())
    }
    fn self_recovery_ready(&mut self) -> Result<(), String> {
        unreachable!()
    }
    fn reload(&mut self) -> Result<(), String> {
        self.asked.push("reload");
        if self.reloads_fail > 0 {
            self.reloads_fail -= 1;
            return Err("Failed to reload daemon: Connection timed out".to_owned());
        }
        Ok(())
    }
    fn gateway_unit_properties(&mut self) -> Result<String, String> {
        self.shown += 1;
        Ok(self.loaded.unwrap_or(LOADED_AS_WRITTEN).to_owned())
    }
    fn try_restart_gateway(&mut self) -> Result<(), String> {
        self.asked.push("try-restart");
        Ok(())
    }
    fn restart(&mut self, _: &str) -> Result<(), String> {
        unreachable!("a round restarts nothing of OpenClaw's")
    }
    fn last_line(&mut self, _: &str) -> Option<String> {
        unreachable!()
    }
    fn harness_unit_properties(&mut self) -> Result<String, String> {
        self.asked.push("show");
        Ok("Id=lamp-update-harness.service\n\
            LoadState=loaded\n\
            FragmentPath=/etc/systemd/system/lamp-update-harness.service\n\
            DropInPaths=\n\
            UnitFileState=static\n"
            .to_owned())
    }
    fn start_harness_unit(&mut self) -> Result<(), String> {
        self.asked.push("start");
        Ok(())
    }
    fn start_writer_unit(&mut self) -> Result<(), String> {
        unreachable!("never the writer's")
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
            "proc",
        ] {
            fs::create_dir_all(root.path().join(folder)).unwrap();
        }
        let lamp = Self { root };
        fs::write(
            lamp.at(hermes_install::MEMINFO),
            format!("MemAvailable:   {} kB\n", 4 * NEEDS_AVAILABLE_KB),
        )
        .unwrap();
        lamp
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
    fn records(&self) -> PathBuf {
        self.root.path().join("var/lib/lampos-update")
    }
    fn at(&self, path: &str) -> PathBuf {
        on(self.root(), path)
    }
    fn harness_there(&self) {
        let bin = self.at("/var/lib/lampos-tasks/.local/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("harness"), "").unwrap();
        symlink(
            "/var/lib/lampos-tasks/.local/bin/harness",
            self.at("/usr/local/bin/harness"),
        )
        .unwrap();
    }
    fn said(&self) -> Option<OpenclawInstall> {
        record(&self.records())
    }
    fn asked(&self) -> Option<Ask> {
        serde_json::from_slice(&fs::read(self.records().join(ASK)).ok()?).ok()
    }
    fn ask(&self) {
        fs::write(self.records().join(ASK), "{\"version\":1}").unwrap();
    }
    fn unit(&self) -> PathBuf {
        unit_path(self.root())
    }
}

const NODE_BYTES: &[u8] = b"a stand-in for node-v24.22.2-linux-arm64.tar.gz";

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// This build's pins, over the stand-in bytes the runner fetches.
fn kit() -> Kit {
    Kit {
        node_sha256: leak(crate::manifest::sha256_hex(NODE_BYTES)),
        ..Kit::BUILT
    }
}

fn own_ids() -> (String, String) {
    let mine = tempfile::tempfile().unwrap().metadata().unwrap();
    (mine.uid().to_string(), mine.gid().to_string())
}

/// A runner that logs each command and answers it: `id` with the test's
/// own ids, `curl -o` by writing the stand-in Node bytes, `tar` by putting
/// a Node of `node` there, `npm ci` by putting the package at `package`,
/// `install -d` by making the folder; `fail_on` makes the first command
/// holding it fail; `main_pid`, what `systemctl show -p MainPID` prints of
/// the gateway's unit (`""`, none running, by default); the rest with
/// nothing.
struct Runs {
    root: PathBuf,
    log: Rc<RefCell<Vec<String>>>,
    node: &'static str,
    package: &'static str,
    fail_on: Option<&'static str>,
    /// What the failing command answers.
    error: &'static str,
    /// The gateway's `MainPID`: `""` or `"0"` when none runs.
    main_pid: &'static str,
    /// How many of the next `ss` looks find the gateway not listening.
    not_listening: RefCell<u32>,
    /// The gateway's start as `ExecMainStartTimestampMonotonic` says it at
    /// each read, in order, the last one kept; none: not said.
    began: RefCell<Vec<String>>,
    /// How many of the next allow-list adds fail because the gateway is
    /// restarted under them (#8444): each makes the gateway inactive for 2
    /// looks, not listening for 3 more, its process started at 1000 s.
    restart_at_add: RefCell<u32>,
    /// How many of the next `systemctl is-active` looks find it inactive.
    inactive: RefCell<u32>,
}

impl Runs {
    fn new(lamp: &Lamp) -> Self {
        Self {
            root: lamp.root().to_owned(),
            log: Rc::default(),
            node: NODE_VERSION,
            package: VERSION,
            fail_on: None,
            error: "refused",
            main_pid: "",
            not_listening: RefCell::new(0),
            began: RefCell::default(),
            restart_at_add: RefCell::new(0),
            inactive: RefCell::new(0),
        }
    }
    fn runner(&self) -> impl FnMut(&[String]) -> Result<String, String> + '_ {
        move |argv: &[String]| {
            let line = argv.join(" ");
            self.log.borrow_mut().push(line.clone());
            if self.fail_on.is_some_and(|wanted| line.contains(wanted)) {
                return Err(self.error.to_owned());
            }
            if line.contains(" approvals allowlist add ") {
                let mut restarts = self.restart_at_add.borrow_mut();
                if *restarts > 0 {
                    *restarts -= 1;
                    *self.inactive.borrow_mut() = 2;
                    *self.not_listening.borrow_mut() = 3;
                    *self.began.borrow_mut() = vec![format!("{}\n", 1000 * 1_000_000)];
                    return Err("gateway closed (1012): service restart".into());
                }
            }
            if line.starts_with("systemctl is-active") {
                let mut inactive = self.inactive.borrow_mut();
                if *inactive > 0 {
                    *inactive -= 1;
                    return Err("inactive".into());
                }
            }
            let (uid, gid) = own_ids();
            if line == format!("systemctl show -p MainPID --value -- {UNIT}") {
                Ok(format!("{}\n", self.main_pid))
            } else if line == "id -g lampos" {
                Ok(format!("{gid}\n"))
            } else if argv[0] == "id" {
                Ok(format!("{uid}\n"))
            } else if argv[0] == "df" {
                Ok(
                    "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                    /dev/mmcblk0p2 60000000 1000 20000000 3% /\n"
                        .to_owned(),
                )
            } else if line.contains("--property=ExecMainStartTimestampMonotonic") {
                let mut began = self.began.borrow_mut();
                let said = began.first().cloned().unwrap_or_default();
                if began.len() > 1 {
                    began.remove(0);
                }
                Ok(said)
            } else if argv[0] == "ss" {
                // The gateway listening, unless a test says otherwise.
                let mut quiet = self.not_listening.borrow_mut();
                if *quiet > 0 {
                    *quiet -= 1;
                    return Ok(String::new());
                }
                Ok(format!("LISTEN 0 511 127.0.0.1:{PORT} 0.0.0.0:*\n"))
            } else if argv[0] == "install" && argv[1] == "-d" {
                fs::create_dir_all(on(&self.root, argv.last().unwrap())).unwrap();
                Ok(String::new())
            } else if argv[0] == "curl" {
                let out = argv.iter().position(|arg| arg == "-o").unwrap() + 1;
                fs::write(&argv[out], NODE_BYTES).unwrap();
                Ok(String::new())
            } else if line.contains(" tar -xzf ") {
                put_node(&self.root, self.node);
                Ok(String::new())
            } else if line.contains(&format!("{NPM_CI_ENV}=on")) {
                put_package(&self.root, self.package);
                Ok(String::new())
            } else if line.ends_with(" --version") {
                // It runs in the namespace script: none there, it fails.
                if fs::metadata(on(&self.root, NS)).is_err() {
                    return Err(format!("{NS}: No such file or directory"));
                }
                Ok("OpenClaw 2026.11.2\n".into())
            } else {
                Ok(String::new())
            }
        }
    }
    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
    fn position(&self, wanted: &str) -> usize {
        let lines = self.lines();
        lines
            .iter()
            .position(|line| line.contains(wanted))
            .unwrap_or_else(|| panic!("no `{wanted}` in {lines:#?}"))
    }
}

fn put_node(root: &Path, version: &str) {
    let header = on(root, NODE_VERSION_H);
    fs::create_dir_all(header.parent().unwrap()).unwrap();
    let mut parts = version.split('.');
    fs::write(
        header,
        format!(
            "#ifndef SRC_NODE_VERSION_H_\n#define NODE_MAJOR_VERSION {}\n#define NODE_MINOR_VERSION {}\n#define NODE_PATCH_VERSION {}\n",
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap()
        ),
    )
    .unwrap();
    fs::create_dir_all(on(root, NODE_BIN)).unwrap();
    fs::write(on(root, NODE), "").unwrap();
}

fn put_package(root: &Path, version: &str) {
    let package = on(root, PACKAGE_JSON);
    fs::create_dir_all(package.parent().unwrap()).unwrap();
    fs::write(
        package,
        format!("{{\"name\":\"openclaw\",\"version\":\"{version}\"}}"),
    )
    .unwrap();
    let bin = on(root, OPENCLAW);
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::write(bin, "").unwrap();
}

fn quiet() -> impl FnMut(&str) {
    |_: &str| {}
}

fn install(lamp: &Lamp, runs: &Runs, at: u64) -> Option<OpenclawInstall> {
    let mut run = runs.runner();
    install_with(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || at,
        &mut quiet(),
        &kit(),
    )
    .unwrap()
}

fn round(lamp: &Lamp, systemd: &mut Fake, skills: bool, at: u64) -> Option<OpenclawInstall> {
    let runs = Runs::new(lamp);
    after_round(
        lamp.root(),
        &lamp.records(),
        sk(skills),
        systemd,
        &mut runs.runner(),
        at,
        Ok(crate::hermes_schedule::Window::Reached),
        &mut quiet(),
    )
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().mode() & 0o7777
}

const AS_LAMPOS: &str = "runuser -u lampos -- env HOME=/var/lib/lampos-openclaw \
     OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state \
     PATH=/var/lib/lampos-openclaw/node/bin:/usr/bin:/bin SHELL=/bin/sh ";

/// On by default on a product lamp, as Hermes, with the names the contract
/// fixes; the skills switch the same. `serve` with it off takes an earlier
/// record and ask away; no ask, nothing runs.
#[test]
fn the_switches_are_on_by_default_on_a_product_lamp_only() {
    const { assert!(PRODUCT_DEFAULT && SKILLS_PRODUCT_DEFAULT) };
    assert_eq!(SWITCH, "LAMP_OPENCLAW_ON_LAMP");
    assert_eq!(SKILLS_SWITCH, "LAMP_OPENCLAW_SKILLS");
    for default in [PRODUCT_DEFAULT, SKILLS_PRODUCT_DEFAULT] {
        assert!(wanted(None, true, default));
        assert!(!wanted(None, false, default));
        assert!(!wanted(Some("off"), true, default));
        assert!(wanted(Some("on"), false, default));
    }
    let lamp = Lamp::new();
    at_start(lamp.root(), &lamp.records(), true, sk(true), 1);
    assert_eq!(lamp.said().unwrap().install, "absent");
    lamp.ask();
    at_start(lamp.root(), &lamp.records(), false, sk(true), 2);
    assert_eq!(lamp.said(), None);
    assert_eq!(lamp.asked(), None);
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 3), None);
    assert!(runs.lines().is_empty());
}

/// With Harness there, a round writes the gateway's unit (reloading
/// systemd), asks and starts Harness's one-shot, `installing`. The one-shot
/// fetches the pinned Node and package as root, checks them, unpacks and
/// installs them as `lampos`, makes the gateway's token once and the
/// config's gateway block, writes the namespace script and the wrapper,
/// root's 0755, and restarts the gateway only if it runs (never enables or
/// starts it): `installed`.
#[test]
fn on_with_harness_there_installs_the_pinned_openclaw_as_lampos() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    let said = round(&lamp, &mut systemd, true, 10).unwrap();
    assert_eq!(said.install, "installing");
    assert_eq!(systemd.asked, ["reload", "reload", "show", "start"]);
    assert_eq!(fs::read_to_string(lamp.unit()).unwrap(), UNIT_TEXT);
    assert_eq!(
        lamp.asked(),
        Some(Ask {
            version: 1,
            remove: false,
            skills: true,
            tasks: false,
            one_path: false,
            by_default: false,
        })
    );
    assert_eq!(round(&lamp, &mut systemd, true, 11), None, "one at a time");

    let runs = Runs::new(&lamp);
    let said = install(&lamp, &runs, 20).unwrap();
    assert_eq!(said.install, "installed", "{:#?}", runs.lines());
    assert_eq!(said.version.as_deref(), Some(VERSION));
    assert_eq!(said.node.as_deref(), Some(NODE_VERSION));
    assert_eq!(lamp.asked(), None, "answered");
    let lines = runs.lines();
    assert!(
        lines.iter().any(|line| line.starts_with(&format!(
            "curl -fsSL --proto =https --tlsv1.2 --max-filesize {MOST_BYTES} -o /"
        )) && line.contains(&format!("/{DOWNLOADS}"))
            && line.ends_with(&format!(" {NODE_URL}"))),
        "{lines:#?}"
    );
    assert_eq!(
        lines.iter().filter(|line| line.starts_with("curl")).count(),
        1,
        "only Node is fetched by lamp-update: npm fetches the locked tree"
    );
    let (uid, gid) = own_ids();
    let ns = format!(
        "LAMP_NS_UID={uid} LAMP_NS_GID={gid} {{}}/usr/bin/unshare --map-root-user --mount --pid --fork --kill-child --mount-proc --propagation private {NS}"
    );
    for wanted in [
        format!("{AS_LAMPOS}tar -xzf "),
        format!(
            "{AS_LAMPOS}{}",
            ns.replace("{}", &format!("{NPM_CI_ENV}=on "))
        ),
        format!("{AS_LAMPOS}{} --version", ns.replace("{}", "")),
        format!("install -d -o lampos -g lampos -m 0700 {PLACE}"),
        format!("install -d -o lampos -g lampos -m 0700 {STATE}"),
        format!("chown -h lampos:lampos {GATEWAY_TOKEN}"),
        format!("chown -h lampos:lampos {CONFIG}"),
        format!("systemctl restart {UNIT}"),
        "systemctl try-restart lamp-tasks.service".to_owned(),
    ] {
        runs.position(&wanted);
    }
    assert!(
        runs.position(NODE_URL) < runs.position(" tar -xzf ")
            && runs.position(" tar -xzf ") < runs.position(NPM_CI_ENV)
            && runs.position(NPM_CI_ENV) < runs.position(" --version")
            && runs.position(" --version") < runs.position(&format!("systemctl restart {UNIT}")),
        "{lines:#?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("runuser") && line.contains(NPM)),
        "npm never runs outside the namespace: {lines:#?}"
    );
    for (path, text) in [(MANIFEST_AT, MANIFEST), (LOCKFILE_AT, LOCKFILE)] {
        assert_eq!(fs::read_to_string(lamp.at(path)).unwrap(), text);
        assert_eq!(mode(&lamp.at(path)), 0o644);
    }
    assert!(
        !lines
            .iter()
            .any(|line| line.starts_with("curl") && line.contains("runuser")),
        "fetched as root, into a root's folder"
    );
    for (path, text) in [
        (WRAPPER, wrapper_text()),
        (NS, ns_text(sk(true))),
        (agent_ns::HOOK_SHIM, agent_ns::HOOK_SHIM_TEXT.to_owned()),
    ] {
        assert_eq!(fs::read_to_string(lamp.at(path)).unwrap(), text);
        assert_eq!(mode(&lamp.at(path)), 0o755);
    }
    let token = fs::read_to_string(lamp.at(GATEWAY_TOKEN)).unwrap();
    let token = token.trim();
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(mode(&lamp.at(GATEWAY_TOKEN)), 0o600);
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.at(CONFIG)).unwrap()).unwrap();
    assert_eq!(
        config,
        serde_json::json!({
            "gateway": {
                "mode": "local",
                "bind": "loopback",
                "port": 18789,
                "auth": {"mode": "token", "token": token},
            },
        }),
        "the gateway block only"
    );
    assert_eq!(mode(&lamp.at(CONFIG)), 0o600);
    assert!(present(lamp.root(), sk(true)));
    assert!(!present(lamp.root(), sk(false)), "the other skills text");

    // Again: the token is made once, the config's other keys kept, nothing
    // fetched.
    fs::write(
        lamp.at(CONFIG),
        serde_json::to_vec(&serde_json::json!({
            "gateway": config["gateway"],
            "models": {"mode": "merge"},
        }))
        .unwrap(),
    )
    .unwrap();
    fs::remove_file(lamp.at(NS)).unwrap();
    lamp.ask();
    let again = Runs::new(&lamp);
    assert_eq!(install(&lamp, &again, 30).unwrap().install, "installed");
    assert!(!again.lines().iter().any(|line| line.starts_with("curl")));
    assert_eq!(
        fs::read_to_string(lamp.at(GATEWAY_TOKEN)).unwrap().trim(),
        token
    );
    let kept: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.at(CONFIG)).unwrap()).unwrap();
    assert_eq!(kept["models"]["mode"], "merge");
    assert!(
        !again
            .lines()
            .iter()
            .any(|line| line.contains("try-restart lamp-tasks")),
        "lamp-tasks is started again only after a new install"
    );
}

/// The wrapper is Hermes's: the forwarder token or nothing; OpenClaw's
/// state folder and the pinned Node's `bin` given. (#6226) One PID
/// namespace for the Gateway and its panes: the gateway's unit makes it
/// (`--pid --fork --kill-child --mount-proc`), every other call joins the
/// running gateway's by its unit's `MainPID`, and with none running a call
/// fails closed, never in a namespace of its own nor in the lamp's.
#[test]
fn the_wrapper_fails_closed_without_the_token_and_runs_only_in_the_namespace() {
    assert!(WRAPPER_TEXT.starts_with("#!/bin/sh\n"));
    assert!(WRAPPER_TEXT.contains(&format!("$(cat {TOKEN} 2>/dev/null)")));
    assert!(WRAPPER_TEXT.contains(&format!(
        "export {TOKEN_ENV}=\"$T\" {STATE_ENV}={STATE} OPENCLAW_NO_AUTO_UPDATE=1 PATH=\"{NODE_BIN}:$PATH\" "
    )));
    assert_eq!(
        NS_FLAGS.join(" "),
        "--map-root-user --mount --pid --fork --kill-child --mount-proc --propagation private"
    );
    // The gateway's path: its own namespaces, only with its unit's switch.
    assert!(WRAPPER_TEXT.contains(&format!(
        "if [ \"$A\" = on ]; then\n  exec /usr/bin/unshare {} {NS} \"$@\"\nfi\n",
        NS_FLAGS.join(" ")
    )));
    assert!(WRAPPER_TEXT.contains(&format!("A=${{{GATEWAY_ENV}-}}\nunset {GATEWAY_ENV}\n")));
    assert!(UNIT_TEXT.contains(&format!(" {GATEWAY_ENV}=on\n")));
    // Every other call: the gateway's namespaces by systemd's MainPID, its
    // PID namespace the one its children are in, then the caller's ids.
    assert!(WRAPPER_TEXT.contains(&format!(
        "P=$(/usr/bin/systemctl show -p MainPID --value {UNIT} 2>/dev/null) || P=\n"
    )));
    // A pid reused after the gateway stopped is not taken: it must still be
    // in the unit's cgroup.
    assert!(
        WRAPPER_TEXT.contains("! grep -q '/lamp-openclaw-gateway\\.service$' \"/proc/$P/cgroup\"")
    );
    assert!(WRAPPER_TEXT.ends_with(&format!(
        "exec /usr/bin/nsenter --user=\"/proc/$P/ns/user\" --mount=\"/proc/$P/ns/mnt\" --pid=\"/proc/$P/ns/pid_for_children\" --preserve-credentials --wd=\"$PWD\" -- /usr/bin/unshare --user --map-user=\"$LAMP_NS_UID\" --map-group=\"$LAMP_NS_GID\" {OPENCLAW} \"$@\"\n"
    )));
    assert_eq!(WRAPPER_TEXT.matches("exec ").count(), 2);
    assert_eq!(
        WRAPPER_TEXT.matches(OPENCLAW).count(),
        1,
        "only in the gateway's namespaces"
    );
    let folder = tempfile::tempdir().unwrap();
    let printer = folder.path().join("unshare");
    fs::write(
        &printer,
        "#!/bin/sh\necho \"token=$LAMP_HERMES_TOKEN state=$OPENCLAW_STATE_DIR path=$PATH gateway=${LAMP_OPENCLAW_GATEWAY-unset}\"\necho \"args: $*\"\n",
    )
    .unwrap();
    fs::set_permissions(&printer, fs::Permissions::from_mode(0o755)).unwrap();
    // systemd's answer: 0 (no gateway), or this test's own pid (a process
    // in the caller's own user namespace, so never a gateway's).
    let systemctl = folder.path().join("systemctl");
    let main_pid = folder.path().join("main-pid");
    fs::write(
        &systemctl,
        format!("#!/bin/sh\ncat {}\n", main_pid.to_string_lossy()),
    )
    .unwrap();
    fs::set_permissions(&systemctl, fs::Permissions::from_mode(0o755)).unwrap();
    let token = folder.path().join("hermes-token");
    let wrapper = folder.path().join("openclaw");
    fs::write(
        &wrapper,
        WRAPPER_TEXT
            .replace(TOKEN, &token.to_string_lossy())
            .replace("/usr/bin/unshare", &printer.to_string_lossy())
            .replace("/usr/bin/nsenter", &printer.to_string_lossy())
            .replace("/usr/bin/systemctl", &systemctl.to_string_lossy()),
    )
    .unwrap();
    let ran = |gateway: bool| {
        let mut command = std::process::Command::new("/bin/sh");
        command
            .arg(&wrapper)
            .args(["gateway", "run"])
            .env("PATH", "/usr/bin:/bin")
            .env_remove(GATEWAY_ENV);
        if gateway {
            command.env(GATEWAY_ENV, "on");
        }
        command.output().unwrap()
    };
    fs::write(&main_pid, "0\n").unwrap();
    let refused = ran(true);
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty(), "nothing run");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("no forwarder token"));
    fs::write(&token, "tok_0123456789abcdef\n").unwrap();
    let good = ran(true);
    assert!(good.status.success(), "{good:?}");
    let out = String::from_utf8_lossy(&good.stdout);
    assert!(out.contains(&format!(
        "token=tok_0123456789abcdef state={STATE} path={NODE_BIN}:/usr/bin:/bin gateway=unset\n"
    )));
    assert!(out.contains(&format!("args: {} {NS} gateway run\n", NS_FLAGS.join(" "))));
    // A pane with no gateway running, or with a "gateway" in its own user
    // namespace: refused, nothing run, never a namespace of its own.
    for pid in [
        "0".to_owned(),
        "".to_owned(),
        "x1".to_owned(),
        std::process::id().to_string(),
    ] {
        fs::write(&main_pid, format!("{pid}\n")).unwrap();
        let pane = ran(false);
        assert!(!pane.status.success(), "{pid}: {pane:?}");
        assert!(pane.stdout.is_empty(), "{pid}: nothing run");
        assert!(
            String::from_utf8_lossy(&pane.stderr)
                .contains("its gateway (lamp-openclaw-gateway.service) is not running"),
            "{pid}: {pane:?}"
        );
    }
}

/// The namespace script is Hermes's, line for line, but for OpenClaw's
/// folder, command and names: the same allow-list and the same steps, with
/// the skills' socket only with the switch on. No Hermes is left in it.
#[test]
fn the_namespace_script_is_hermess_for_openclaw() {
    for (from, _) in ENGINE_EDITS {
        assert!(
            hermes_install::ns_text(true, false).contains(from),
            "`{from}` is not in Hermes's script"
        );
    }
    for skills in [false, true] {
        let with_mode = ns_text(sk(skills));
        assert_eq!(with_mode.matches(NPM_CI_MODE).count(), 1);
        assert!(with_mode.ends_with(&format!("{NPM_CI_MODE}{GATEWAY_RUN}{RUN_LINE}")));
        let checked = std::process::Command::new("/bin/sh")
            .args(["-n", "-c", &with_mode])
            .output()
            .unwrap();
        assert!(checked.status.success(), "{checked:?}");
        let ours = with_mode
            .replacen(NPM_CI_MODE, "", 1)
            .replacen(GATEWAY_RUN, "", 1);
        let hermes = hermes_install::ns_text(skills, false);
        assert!(!ours.to_ascii_lowercase().contains("hermes"), "{ours}");
        assert!(!ours.contains("#5978") && !ours.contains("#6128"));
        assert_eq!(ours.lines().count(), hermes.lines().count());
        for (mine, theirs) in ours.lines().zip(hermes.lines()) {
            if !theirs.to_ascii_lowercase().contains("hermes") && !theirs.contains("#5978") {
                assert_eq!(mine, theirs);
            }
        }
        assert!(ours.contains(&format!(
            "H={PLACE} W={WORK} K={SKILLS}{} S=/tmp/.lampos-ns\n",
            if skills {
                format!(" C={SKILLS_SOCKET}")
            } else {
                String::new()
            }
        )));
        assert!(ours.ends_with(&format!(
            "exec /usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" {OPENCLAW} \"$@\"\n"
        )));
        assert!(ours.contains(&format!("only inside the namespace {WRAPPER} makes")));
        assert_eq!(ours.contains(SKILLS_SOCKET), skills);
    }
    assert!(ns_text(sk(true)).contains("# LAMP_OPENCLAW_SKILLS=on (#6226)"));
    assert_eq!(ALLOWED, [PLACE, WORK, SKILLS]);
}

/// (#8289) Every namespace script runs the unit's `gateway run` through
/// [`GATEWAY_RUN`], once, right before [`RUN_LINE`], and starts there the
/// same OpenClaw [`RUN_LINE`] execs; the unit still starts `gateway run`
/// and its `MainPID` is still the `unshare` with [`NS_FLAGS`].
#[test]
fn the_gateway_runs_as_a_child_of_the_namespaces_shell() {
    for skills in [false, true] {
        for tasks in [false, true] {
            for one_path in [false, true] {
                let text = ns_text(Shown {
                    skills,
                    tasks,
                    one_path,
                });
                assert_eq!(text.matches(GATEWAY_RUN).count(), 1);
                assert!(text.ends_with(&format!("{GATEWAY_RUN}{RUN_LINE}")));
            }
        }
    }
    let run = RUN_LINE
        .strip_prefix("exec ")
        .unwrap()
        .trim_end_matches('\n');
    assert!(GATEWAY_RUN.contains(&format!("  {run} &\n")));
    assert!(GATEWAY_RUN.contains("if [ \"${1-}\" = gateway ] && [ \"${2-}\" = run ]; then\n"));
    assert!(UNIT_TEXT.contains("ExecStart=/usr/local/bin/openclaw gateway run\n"));
    assert!(WRAPPER_TEXT.contains(&format!(
        "exec /usr/bin/unshare {} {NS} \"$@\"\n",
        NS_FLAGS.join(" ")
    )));
}

/// (#8289) [`GATEWAY_RUN`] run by `sh` (dash where there is one, as on a
/// lamp) with a stand-in OpenClaw: a SIGTERM to the shell ends a gateway
/// with no handler of its own and one that stops on it, the shell exiting
/// 0 (a clean stop) within a few seconds; a gateway that fails its stop or
/// exits by itself gives its own status; any other call falls through to
/// [`RUN_LINE`]. The kernel's PID 1 rule itself needs a PID namespace (a
/// lamp's row).
#[test]
fn the_gateway_mode_passes_a_stop_on_and_exits_as_the_gateway_does() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let sh = ["/bin/dash", "/bin/sh"]
        .into_iter()
        .find(|sh| Path::new(sh).exists())
        .unwrap();
    let run = RUN_LINE.strip_prefix("exec ").unwrap();
    let run = run.strip_suffix(" \"$@\"\n").unwrap();
    let cases = [
        ("no-handler", "exec sleep 30", true, 0),
        (
            "stops",
            "trap 'exit 0' TERM\nwhile :; do sleep 0.1; done",
            true,
            0,
        ),
        (
            "fails-its-stop",
            "trap 'exit 1' TERM\nwhile :; do sleep 0.1; done",
            true,
            1,
        ),
        ("exits-by-itself", "sleep 0.3\nexit 3", false, 3),
    ];
    for (name, body, stop, want) in cases {
        let child = dir.path().join(name);
        std::fs::write(&child, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = GATEWAY_RUN.replacen(run, &child.to_string_lossy(), 1) + "exit 99\n";
        let started = Instant::now();
        let mut shell = std::process::Command::new(sh)
            .args(["-c", &script, "openclaw-ns", "gateway", "run"])
            .spawn()
            .unwrap();
        // Past the stand-in's own start, as a lamp's stop after a start is.
        std::thread::sleep(Duration::from_millis(700));
        if stop {
            let pid = shell.id().to_string();
            let sent = std::process::Command::new("kill")
                .args(["-TERM", &pid])
                .status()
                .unwrap();
            assert!(sent.success());
        }
        let status = shell.wait().unwrap();
        assert_eq!(status.code(), Some(want), "{name}");
        assert!(started.elapsed() < Duration::from_secs(10), "{name}");
    }
    let other = std::process::Command::new(sh)
        .args([
            "-c",
            &format!("{GATEWAY_RUN}exit 99\n"),
            "openclaw-ns",
            "--version",
        ])
        .status()
        .unwrap();
    assert_eq!(other.code(), Some(99));
}

/// The pins: Node 24 at or past 24.16 (OpenClaw's floor), nodejs.org's
/// linux-arm64 tarball by its SHA-256; the package Harness measured, the
/// registry's tarball by its sha512 integrity.
#[test]
fn the_pins_are_node_24_and_the_measured_package() {
    let parts: Vec<u32> = NODE_VERSION
        .split('.')
        .map(|part| part.parse().unwrap())
        .collect();
    assert!(parts[0] == 24 && parts[1] >= 16, "{NODE_VERSION}");
    for url in [NODE_URL, NODE_ORIGIN] {
        assert!(
            url.starts_with("https://")
                && url.ends_with(&format!(
                    "/v{NODE_VERSION}/node-v{NODE_VERSION}-linux-arm64.tar.gz"
                )),
            "{url}"
        );
    }
    assert!(NODE_URL.starts_with("https://cdn.autonomous.ai/harness/runtime/node/"));
    assert!(NODE_SHA256.len() == 64 && NODE_SHA256.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(VERSION, "2026.11.2");
    assert_eq!(
        PACKAGE_URL,
        format!("https://registry.npmjs.org/openclaw/-/openclaw-{VERSION}.tgz")
    );
    assert!(PACKAGE_INTEGRITY.starts_with("sha512-") && PACKAGE_INTEGRITY.len() == 95);
    assert_eq!(
        integrity_of(b""),
        "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg=="
    );
}

/// The lockfile shipped is the pinned one: its SHA-256, a project that
/// wants `openclaw` at [`VERSION`] exactly (as [`MANIFEST`] does), and its
/// `openclaw` the registry tarball at [`PACKAGE_URL`] with
/// [`PACKAGE_INTEGRITY`]; every other package fetched from the registry
/// with a sha512 integrity, or bundled in one that is.
#[test]
fn the_lockfile_pins_the_whole_tree_and_the_measured_package() {
    assert_eq!(
        crate::manifest::sha256_hex(LOCKFILE.as_bytes()),
        LOCKFILE_SHA256
    );
    lockfile_pinned(LOCKFILE, LOCKFILE_SHA256, VERSION).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(
        manifest["dependencies"],
        serde_json::json!({"openclaw": VERSION})
    );
    assert_eq!(manifest["private"], true);
    let lock: serde_json::Value = serde_json::from_str(LOCKFILE).unwrap();
    assert_eq!(lock["name"], manifest["name"]);
    assert!(lock["packages"].as_object().unwrap().len() > 100);

    let refused = |text: &str, version: &str| {
        let sha = crate::manifest::sha256_hex(text.as_bytes());
        lockfile_pinned(text, &sha, version).unwrap_err()
    };
    assert!(
        lockfile_pinned(LOCKFILE, &"0".repeat(64), VERSION)
            .unwrap_err()
            .contains("sha256")
    );
    assert!(refused(LOCKFILE, "2026.11.1").contains("wants"));
    let other = LOCKFILE.replacen(PACKAGE_INTEGRITY, "sha512-other", 1);
    assert!(refused(&other, VERSION).contains("the lockfile's openclaw"));
    let mut loose = lock.clone();
    loose["packages"]["node_modules/zod"]
        .as_object_mut()
        .unwrap()
        .remove("integrity");
    assert!(refused(&loose.to_string(), VERSION).contains("node_modules/zod"));
}

/// A lockfile that is not the pinned one is `pin_mismatch` before anything
/// is fetched or written.
#[test]
fn a_lockfile_not_the_pinned_one_is_refused_before_any_fetch() {
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs::new(&lamp);
    let other = Kit {
        lockfile: leak(LOCKFILE.replacen(PACKAGE_INTEGRITY, "sha512-other", 1)),
        ..kit()
    };
    let said = install_with(
        lamp.root(),
        &lamp.records(),
        &mut runs.runner(),
        &mut || 6,
        &mut quiet(),
        &other,
    )
    .unwrap()
    .unwrap();
    assert_eq!(said.error.as_deref(), Some("pin_mismatch"));
    assert!(!runs.lines().iter().any(|line| line.starts_with("curl")));
    assert!(!lamp.at(LOCKFILE_AT).exists() && !lamp.at(NS).exists());
}

/// A Node whose bytes are not the pinned ones is `pin_mismatch`: nothing
/// unpacked or installed, no entry point, the gateway disabled.
#[test]
fn bytes_not_the_pinned_ones_are_refused() {
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs::new(&lamp);
    let mut run = runs.runner();
    let said = install_now(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 5,
        &mut quiet(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(said.error.as_deref(), Some("pin_mismatch"));
    assert!(!runs.lines().iter().any(|line| line.contains(" tar ")));
    assert!(!lamp.at(WRAPPER).exists() && !lamp.at(NS).exists());
}

/// A failure at any later step takes the entry points out and stops and
/// a running gateway; its state stays.
#[test]
fn a_failed_install_takes_the_entry_points_out_and_stops_the_gateway() {
    for (fail_on, error) in [
        (NODE_URL, "fetch"),
        (" tar -xzf ", "installer"),
        (NPM_CI_ENV, "installer"),
        ("systemctl restart lamp-openclaw", "gateway"),
        ("--map-root-user", "no_userns"),
        ("command -v tar", "no_tools"),
    ] {
        let lamp = Lamp::new();
        fs::write(lamp.unit(), UNIT_TEXT).unwrap();
        fs::write(lamp.at(WRAPPER), "old").unwrap();
        fs::write(lamp.at(NS), "old").unwrap();
        lamp.ask();
        let runs = Runs {
            fail_on: Some(fail_on),
            ..Runs::new(&lamp)
        };
        let said = install(&lamp, &runs, 7).unwrap();
        assert_eq!(said.error.as_deref(), Some(error), "{fail_on}");
        assert!(
            !lamp.at(WRAPPER).exists() && !lamp.at(NS).exists(),
            "{fail_on}"
        );
        runs.position(&format!("systemctl stop {UNIT}"));
    }
    // A Node or package not at the pin after its step: version.
    for (node, package) in [("24.22.1", VERSION), (NODE_VERSION, "2026.11.1")] {
        let lamp = Lamp::new();
        lamp.ask();
        let runs = Runs {
            node,
            package,
            ..Runs::new(&lamp)
        };
        assert_eq!(
            install(&lamp, &runs, 8).unwrap().error.as_deref(),
            Some("version")
        );
    }
}

/// #8225 (lamp-52e6, 21 Nov 15:39): the install's `gateway` block check
/// joined the running gateway, which restarted under it, and the check was
/// killed ("...: Killed" from `runuser`). A check ended by a signal or past
/// its bound gave no verdict: the live config stays as it was, nothing is
/// taken out, the record is not `failed`/`config`, and the line says the
/// next round tries again. A check that ran and exited non-zero is still a
/// refusal: `failed`/`config`, the entry points taken out.
#[test]
fn an_install_check_killed_or_timed_out_takes_nothing_out_and_a_refusal_still_fails() {
    let theirs = r#"{"models": {"mode": "merge"}}"#;
    let set_up = || {
        let lamp = Lamp::new();
        fs::write(lamp.unit(), UNIT_TEXT).unwrap();
        fs::create_dir_all(lamp.at(STATE)).unwrap();
        fs::write(lamp.at(CONFIG), theirs).unwrap();
        fs::write(lamp.at(WRAPPER), "old").unwrap();
        fs::write(lamp.at(NS), "old").unwrap();
        fs::write(lamp.at(SKILL_START), "old").unwrap();
        write(&lamp.records(), OpenclawInstall::installed(1));
        lamp.ask();
        lamp
    };
    let check = "runuser -u lampos -- env OPENCLAW_CONFIG_PATH=/x /usr/local/bin/openclaw config validate --json";
    for error in [
        leak(format!("{check}: Killed")),
        leak(format!("{check}: some output\nTerminated")),
        leak(format!(
            "{check}: update query ended with signal: 9 (SIGKILL)"
        )),
        leak(format!(
            "{check}: did not end within 90 s: it ran on, or a child it left held its output"
        )),
        "OpenClaw's own config check refused it (status killed): ",
        "OpenClaw's own config check ran past 60 s",
    ] {
        let lamp = set_up();
        let runs = Runs {
            fail_on: Some("config validate --json"),
            error,
            ..Runs::new(&lamp)
        };
        let mut lines = Vec::new();
        let said = install_with(
            lamp.root(),
            &lamp.records(),
            &mut runs.runner(),
            &mut || 7,
            &mut |line: &str| lines.push(line.to_owned()),
            &kit(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(said.install, "installed", "{error}: {said:?}");
        assert_eq!(lamp.said(), Some(said), "{error}");
        assert_eq!(
            fs::read_to_string(lamp.at(CONFIG)).unwrap(),
            theirs,
            "{error}"
        );
        for path in [WRAPPER, NS, SKILL_START] {
            assert!(lamp.at(path).exists(), "{path} taken out on {error}");
        }
        assert!(
            !runs
                .lines()
                .iter()
                .any(|line| line == &format!("systemctl stop {UNIT}")),
            "{error}"
        );
        assert!(
            lines.iter().any(
                |line| line.contains("OpenClaw's config check did not finish (")
                    && line
                        .contains("): not checked now, kept as it was; the next round tries again")
            ),
            "{error}: {lines:#?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("refused it:")),
            "{error}: {lines:#?}"
        );
    }

    // OpenClaw's own verdict: a refusal, as before.
    let lamp = set_up();
    let runs = Runs {
        fail_on: Some("config validate --json"),
        error: "models.mode: Unrecognized key",
        ..Runs::new(&lamp)
    };
    let said = install(&lamp, &runs, 8).unwrap();
    assert_eq!(
        (said.install.as_str(), said.error.as_deref()),
        ("failed", Some("config"))
    );
    assert_eq!(fs::read_to_string(lamp.at(CONFIG)).unwrap(), theirs);
    assert!(!lamp.at(WRAPPER).exists() && !lamp.at(NS).exists());
}

/// Off: a round asks the one-shot to stop the gateway and take the entry
/// points out; once they are gone, the round takes the unit out itself.
#[test]
fn the_switch_off_stops_the_gateway_and_takes_everything_out() {
    let lamp = Lamp::new();
    lamp.harness_there();
    fs::write(lamp.unit(), UNIT_TEXT).unwrap();
    fs::write(lamp.at(WRAPPER), wrapper_text()).unwrap();
    fs::write(lamp.at(NS), ns_text(sk(true))).unwrap();
    let mut systemd = Fake::default();
    assert!(after_round_off(
        lamp.root(),
        &lamp.records(),
        &mut systemd,
        &mut quiet()
    ));
    assert!(lamp.asked().unwrap().remove);
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 9), None);
    assert_eq!(
        runs.lines()[0],
        format!("systemctl stop {UNIT}"),
        "the gateway stopped first"
    );
    assert!(!lamp.at(WRAPPER).exists() && !lamp.at(NS).exists());
    assert!(lamp.unit().exists(), "the round's to take out");
    let mut systemd = Fake::default();
    assert!(!after_round_off(
        lamp.root(),
        &lamp.records(),
        &mut systemd,
        &mut quiet()
    ));
    assert!(!lamp.unit().exists());
    assert_eq!(systemd.asked, ["stop-gateway", "reload"]);
}

/// The gateway's unit: through the wrapper as `lampos`, from OpenClaw's
/// state, started again whenever it exits, below the lamp's services with
/// caps.
#[test]
fn the_gateway_unit_runs_through_the_wrapper_as_lampos_with_caps() {
    let lines: Vec<&str> = UNIT_TEXT.lines().collect();
    for wanted in [
        "User=lampos",
        "Group=lampos",
        "ExecStart=/usr/local/bin/openclaw gateway run",
        "WorkingDirectory=/var/lib/lampos-openclaw/state",
        "Restart=always",
        "RestartSec=10",
        // (#8320) room for a first-start drain (~80 s) before a SIGKILL
        "TimeoutStopSec=150",
        "Nice=10",
        "CPUQuota=100%",
        "Environment=HOME=/var/lib/lampos-openclaw OPENCLAW_STATE_DIR=/var/lib/lampos-openclaw/state LAMP_OPENCLAW_GATEWAY=on",
        "MemoryMax=2500M",
        "[Install]",
        "WantedBy=multi-user.target",
        "NoNewPrivileges=yes",
    ] {
        assert!(lines.contains(&wanted), "{wanted} missing");
    }
    // (#6226: RAM floor) Node's heap limit left as it is.
    assert!(!UNIT_TEXT.contains("NODE_OPTIONS") && UNIT_TEXT.contains("(#6226: RAM floor)"));
    // (the owner on #6226) always on: no memory throttle, only the guard.
    assert!(!UNIT_TEXT.contains("MemoryHigh"));
    assert_eq!(
        WANTS,
        format!("/etc/systemd/system/multi-user.target.wants/{UNIT}")
    );
    assert_eq!(UNIT, "lamp-openclaw-gateway.service");
    assert!(UNIT_TEXT.contains(&format!("port {PORT}")));
}

/// #8238: OpenClaw 2026.11.2 answers a change of its `gateway` keys with a
/// "full process restart (supervisor restart)" that exits 0, which
/// `Restart=on-failure` took as done and left the gateway down (lamp-52e6,
/// 21 Nov 21:18). The unit starts it again on any exit; a crash loop still
/// stops at the start limit (in `[Unit]`, where systemd reads it), and a
/// `systemctl stop` (every stop lamp-update and lamp-setup make) still
/// stops it, as systemd never restarts a unit stopped by a job.
#[test]
fn the_gateway_starts_again_after_its_own_supervisor_restart() {
    for text in [UNIT_TEXT.to_owned(), unit_text_for(one_path())] {
        let restart: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("Restart="))
            .collect();
        assert_eq!(restart, ["Restart=always"], "{text}");
        let unit = &text[..text.find("[Service]").unwrap()];
        assert!(unit.contains("\nStartLimitIntervalSec=600\n"), "{text}");
        assert!(unit.contains("\nStartLimitBurst=5\n"), "{text}");
    }
}

/// #8238: a lamp running the earlier unit (`Restart=on-failure`, on
/// stable since 11 Nov) gets the new text from its next round: the bytes
/// differ, so the round writes it whole and reloads systemd, which a
/// running gateway's restart policy follows with no restart of its own.
/// OpenClaw isn't installed here, so nothing is started; a gateway left
/// down on a lamp with it: `a_round_starts_a_gateway_left_down_only_while_it_is_wanted`.
#[test]
fn an_earlier_on_failure_unit_is_written_again_by_the_next_round() {
    let lamp = Lamp::new();
    let earlier = UNIT_TEXT.replace("Restart=always\n", "Restart=on-failure\n");
    assert_ne!(earlier, UNIT_TEXT);
    fs::write(lamp.unit(), &earlier).unwrap();
    let mut systemd = Fake::default();
    round_said(&lamp, &mut systemd);
    assert_eq!(fs::read_to_string(lamp.unit()).unwrap(), UNIT_TEXT);
    assert_eq!(systemd.asked, ["reload"], "reloaded, nothing restarted");
}

/// #8238 (the first builder's 21:56, Migrate's and Twelve's HOLDs): a
/// gateway the bug already left down is started by the next round while
/// [`gateway_wanted`] says wanted (always with the switch off), one past its
/// start limit after its `reset-failed`, said once; never while not wanted
/// (#8105's stop), and a running one gets nothing.
#[test]
fn a_round_starts_a_gateway_left_down_only_while_it_is_wanted() {
    let lamp = Lamp::new();
    lamp.harness_there();
    round(&lamp, &mut Fake::default(), true, 1);
    assert_eq!(
        install(&lamp, &Runs::new(&lamp), 2).unwrap().install,
        "installed"
    );
    let hermes = Said {
        default: Some("hermes".to_owned()),
    };
    let at = |gateway: Gateway, state: &'static str, at: u64| {
        let mut systemd = Fake {
            gateway: Some(state),
            ..Fake::default()
        };
        let mut said = Vec::new();
        after_round_with(
            lamp.root(),
            &lamp.records(),
            sk(true),
            gateway,
            true,
            &mut systemd,
            &mut Runs::new(&lamp).runner(),
            at,
            Ok(crate::hermes_schedule::Window::Reached),
            &mut |line: &str| said.push(line.to_owned()),
        );
        (systemd.asked, said)
    };
    let off = gateway_wanted(false, None);
    let (asked, _) = at(off, "inactive", 3);
    assert!(
        asked.ends_with(&["gateway-state", "start-gateway"]),
        "{asked:?}"
    );
    let (asked, said) = at(off, "failed", 4);
    assert!(
        asked.ends_with(&["gateway-state", "reset-failed", "start-gateway"]),
        "{asked:?}"
    );
    assert_eq!(
        said.iter()
            .filter(|line| line.contains("reset-failed"))
            .count(),
        1,
        "{said:?}"
    );
    let (asked, _) = at(off, "active", 5);
    assert!(!asked.contains(&"start-gateway"), "{asked:?}");
    // The switch on, OpenClaw not the default: left down.
    let (asked, _) = at(gateway_wanted(true, Some(hermes)), "inactive", 6);
    assert!(!asked.contains(&"start-gateway"), "{asked:?}");
    let (asked, _) = at(gateway_wanted(true, None), "failed", 7);
    assert!(!asked.contains(&"start-gateway"), "{asked:?}");
    assert!(!asked.contains(&"reset-failed"), "{asked:?}");
}

/// #8238 (Thirteen's HOLD, the first builder's 05:02): with the switch on,
/// the 10 s look between rounds leaves a `failed` gateway to the next
/// round, which is the only one to reset it, so `StartLimitBurst` still
/// ends a crash loop between rounds.
#[test]
fn only_a_round_resets_a_failed_gateway() {
    let lamp = Lamp::new();
    lamp.harness_there();
    round(&lamp, &mut Fake::default(), true, 1);
    assert_eq!(
        install(&lamp, &Runs::new(&lamp), 2).unwrap().install,
        "installed"
    );
    round(&lamp, &mut Fake::default(), true, 3);
    let wanted = gateway_wanted(
        true,
        Some(Said {
            default: Some("openclaw".to_owned()),
        }),
    );
    assert_eq!(wanted, Gateway::Wanted);
    for _ in 0..2 {
        let mut systemd = Fake {
            gateway: Some("failed"),
            active: Some("inactive"),
            ..Fake::default()
        };
        let mut said = Vec::new();
        keep_between_rounds(
            lamp.root(),
            &mut systemd,
            wanted,
            false,
            &mut Runs::new(&lamp).runner(),
            &mut |line: &str| said.push(line.to_owned()),
        );
        assert_eq!(systemd.asked, ["state", "gateway-state"]);
        assert!(said.is_empty(), "{said:?}");
    }
    let mut systemd = Fake {
        gateway: Some("failed"),
        ..Fake::default()
    };
    after_round_with(
        lamp.root(),
        &lamp.records(),
        sk(true),
        wanted,
        true,
        &mut systemd,
        &mut Runs::new(&lamp).runner(),
        4,
        Ok(crate::hermes_schedule::Window::Reached),
        &mut quiet(),
    );
    assert!(
        systemd
            .asked
            .ends_with(&["gateway-state", "reset-failed", "start-gateway"]),
        "{:?}",
        systemd.asked
    );
}

/// #8238 (the first builder's 04:59): a first install whose gateway hit
/// its start limit (`failed`, which refuses a restart until reset) resets
/// it before its restart, and is installed, not failed at "gateway".
#[test]
fn a_first_install_resets_a_gateway_past_its_start_limit_before_its_restart() {
    let lamp = Lamp::new();
    fs::write(lamp.unit(), UNIT_TEXT).unwrap();
    lamp.ask();
    let runs = Runs::new(&lamp);
    let mut inner = runs.runner();
    let mut reset = false;
    let mut run = |argv: &[String]| {
        let line = argv.join(" ");
        reset |= line == format!("systemctl reset-failed {UNIT}");
        if line == format!("systemctl restart {UNIT}") && !reset {
            return Err(format!(
                "Job for {UNIT} failed. See \"systemctl status\": start request repeated too quickly"
            ));
        }
        inner(argv)
    };
    let said = install_with(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 1,
        &mut quiet(),
        &kit(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        (said.install.as_str(), said.error.as_deref()),
        ("installed", None)
    );
    drop(inner);
    assert!(
        runs.position(&format!("systemctl reset-failed {UNIT}"))
            < runs.position(&format!("systemctl restart {UNIT}"))
    );
}

/// (#6226) npm hung 38 min at its extract on lamp-c0a4 with no output
/// flags: `npm ci` runs with `--loglevel=http` (and no install scripts),
/// and each long step under a hard 20 min bound. One that runs past it
/// ends `failed` with `timeout`, never `installing` for good.
#[test]
fn npm_ci_runs_with_output_and_a_long_step_past_its_bound_is_timeout() {
    assert!(NPM_CI_MODE.contains(&format!(
        "{NODE} {NPM} ci --omit=dev --ignore-scripts --no-audit --no-fund --no-update-notifier --loglevel=http\n"
    )));
    assert!(NPM_CI_MODE.contains(&format!("cd {PREFIX} ")));
    let lamp = Lamp::new();
    fs::write(lamp.unit(), UNIT_TEXT).unwrap();
    lamp.ask();
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 1).unwrap().install, "installed");
    for step in [NODE_URL, " tar -xzf ", NPM_CI_ENV] {
        let lamp = Lamp::new();
        lamp.ask();
        let runs = Runs {
            fail_on: Some(step),
            error: "runuser ... did not end within 1200 s: stopped",
            ..Runs::new(&lamp)
        };
        let said = install(&lamp, &runs, 2).unwrap();
        assert_eq!(
            (said.install.as_str(), said.error.as_deref()),
            ("failed", Some("timeout")),
            "{step}"
        );
        assert_eq!(lamp.said(), Some(said));
    }
    assert_eq!(STEP_WITHIN, Duration::from_secs(20 * 60));
}

/// (#6226) OpenClaw needs Node >=24.16 (<25) or >=26.1: the lamp's own
/// pinned Node is one, and a Node below it found after the unpacking is
/// refused as `old_node`, named, with nothing put in reach of `lampos`.
#[test]
fn a_node_below_what_openclaw_runs_on_is_old_node() {
    assert!(node_supported(NODE_VERSION));
    for good in ["24.16.0", "24.22.2", "26.1.0", "27.0.0"] {
        assert!(node_supported(good), "{good}");
    }
    for bad in ["24.15.9", "22.12.0", "25.0.0", "26.0.0", "24", "x.y.z"] {
        assert!(!node_supported(bad), "{bad}");
    }
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs {
        node: "24.15.0",
        ..Runs::new(&lamp)
    };
    let mut lines = Vec::new();
    let said = install_with(
        lamp.root(),
        &lamp.records(),
        &mut runs.runner(),
        &mut || 3,
        &mut |line: &str| lines.push(line.to_owned()),
        &kit(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(said.error.as_deref(), Some("old_node"));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("24.15.0, below what OpenClaw runs on")),
        "{lines:?}"
    );
    assert!(!runs.lines().iter().any(|line| line.contains(NPM_CI_ENV)));
    assert!(!lamp.at(WRAPPER).exists());
}

/// (#6226) The wrapper the lamp runs, as tried by hand on lamp-c0a4:
/// the pinned Node first on `PATH`, OpenClaw's state folder, the forwarder
/// token from its file, root's 0755, and each path ending in `exec`, so no
/// shell of the wrapper stays above OpenClaw.
#[test]
fn the_wrapper_puts_the_lamps_node_first_and_ends_in_exec() {
    assert!(WRAPPER_TEXT.contains(&format!("PATH=\"{NODE_BIN}:$PATH\"")));
    assert_eq!(NODE_BIN, "/var/lib/lampos-openclaw/node/bin");
    assert!(WRAPPER_TEXT.contains(&format!("{STATE_ENV}={STATE}")));
    assert!(WRAPPER_TEXT.contains(&format!("{TOKEN_ENV}=\"$T\"")));
    let last = WRAPPER_TEXT.lines().last().unwrap();
    assert!(last.starts_with("exec "), "{last}");
    let lamp = Lamp::new();
    fs::write(lamp.unit(), UNIT_TEXT).unwrap();
    lamp.ask();
    let runs = Runs::new(&lamp);
    install(&lamp, &runs, 4).unwrap();
    assert_eq!(
        fs::read_to_string(lamp.at(WRAPPER)).unwrap(),
        wrapper_text()
    );
    assert_eq!(mode(&lamp.at(WRAPPER)), 0o755);
}

/// Each fetch, the unpacking and `npm ci` get [`LONG_STEP_WITHIN`]
/// ([`STEP_WITHIN`] and the most it may be frozen, as Hermes's stage);
/// OpenClaw's other steps the plain bound.
#[test]
fn openclaw_s_long_steps_get_the_long_bound() {
    let downloads = Downloads::make().unwrap();
    let file = downloads.file("node.tar.gz").to_string_lossy().into_owned();
    let unpack = as_lampos(&["tar", "-xzf", file.as_str()]);
    assert!(runs_long_step(&unpack));
    assert_eq!(install::bound_of(&unpack), LONG_STEP_WITHIN);
    let npm = in_namespace("1000", "1000", true, &[]);
    assert!(runs_long_step(&npm));
    assert_eq!(install::bound_of(&npm), LONG_STEP_WITHIN);
    assert_eq!(
        LONG_STEP_WITHIN,
        STEP_WITHIN + crate::hermes_schedule::MOST_FROZEN
    );
    assert!(
        LONG_STEP_WITHIN <= Duration::from_secs(3600),
        "the one-shot's hour"
    );
    let version = in_namespace("1000", "1000", false, &["--version"]);
    assert!(!runs_long_step(&version));
    assert_eq!(install::bound_of(&version), crate::world::RESTART_WITHIN);
}

/// The one-shot installs OpenClaw after Hermes, whatever Hermes's did.
#[test]
fn the_one_shot_installs_openclaw_after_hermes() {
    let lamp = Lamp::new();
    lamp.harness_there();
    fs::write(lamp.records().join(hermes_install::ASK), "{\"version\":1}").unwrap();
    lamp.ask();
    let runs = Runs::new(&lamp);
    let mut run = runs.runner();
    hermes_install::one_shot(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 3,
        &mut quiet(),
    )
    .unwrap();
    let hermes = runs.position(hermes_install::INSTALLER_FILE);
    let openclaw = runs.position(NODE_URL);
    assert!(hermes < openclaw, "{:#?}", runs.lines());
    assert!(hermes_install::record(&lamp.records()).is_some());
    assert!(lamp.said().is_some());
}

/// Turns: Hermes's install under way leaves OpenClaw's ask for the same
/// one-shot (#7019), and OpenClaw's holds Hermes's; one stuck past its
/// bound holds nothing.
#[test]
fn the_two_installs_take_turns_in_the_one_shot() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let hermes = serde_json::json!({"install": "installing", "revision": 1, "at_unix_ms": 100});
    fs::write(
        lamp.records().join(hermes_install::RECORD),
        hermes.to_string(),
    )
    .unwrap();
    let mut systemd = Fake::default();
    assert_eq!(round(&lamp, &mut systemd, false, 200), None);
    assert!(lamp.asked().is_some(), "asked, for after Hermes's");
    assert!(!systemd.asked.contains(&"start"));
    let stuck = 100 + hermes_install::STUCK_AFTER.as_millis() as u64;
    assert_eq!(
        round(&lamp, &mut systemd, false, stuck).unwrap().install,
        "installing"
    );
    assert!(lamp.asked().is_some());

    // And the other way: OpenClaw's under way holds Hermes's.
    fs::remove_file(lamp.records().join(hermes_install::RECORD)).unwrap();
    let held = hermes_install::after_round(
        lamp.root(),
        &lamp.records(),
        hermes_install::Shown::default(),
        &mut systemd,
        stuck + 1,
        Ok(crate::hermes_schedule::Window::Reached),
        &mut quiet(),
    );
    assert_eq!(held, None);
    assert!(!lamp.records().join(hermes_install::ASK).exists());
}

/// #7019: with Hermes's install under way, OpenClaw's ask is still written,
/// so the one-shot running Hermes's reads it after Hermes and lays out
/// OpenClaw's wrapper; no second one-shot is started.
#[test]
fn openclaw_asks_while_hermes_s_install_is_under_way() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let hermes = serde_json::json!({"install": "installing", "revision": 1, "at_unix_ms": 100});
    fs::write(
        lamp.records().join(hermes_install::RECORD),
        hermes.to_string(),
    )
    .unwrap();
    let mut systemd = Fake::default();
    assert_eq!(round(&lamp, &mut systemd, false, 200), None);
    assert!(lamp.asked().is_some(), "OpenClaw's ask is written");
    assert!(!systemd.asked.contains(&"start"), "no second one-shot");
}

/// Heron's HOLD on #7942: `npm ci` paused for a hot board 2 min in, kept
/// frozen 38 min (past [`STEP_WITHIN`], under `MOST_FROZEN`), then 3 min
/// more to finish: the watch never stops it, and the step's 43 min are
/// inside its bound, so it is not recorded `timeout` on the thaw.
#[test]
fn a_long_step_frozen_past_20_min_for_heat_is_not_timed_out() {
    use crate::hermes_schedule::{Act, Lease, Watch};
    const MIN: u64 = 60_000;
    let lease = Lease::Fresh {
        boot: "a".into(),
        physical: Some(false),
        no_slide: false,
    };
    let npm = in_namespace("1000", "1000", true, &[]);
    let mut watch = Watch::default();
    assert_eq!(watch.look_with_heat(0, Ok(()), false, &lease), Act::Nothing);
    assert_eq!(
        watch.look_with_heat(2 * MIN, Ok(()), true, &lease),
        Act::Pause
    );
    for at in (3..40).map(|minute| minute * MIN) {
        assert_eq!(watch.look_with_heat(at, Ok(()), true, &lease), Act::Nothing);
    }
    assert_eq!(
        watch.look_with_heat(40 * MIN, Ok(()), false, &lease),
        Act::Resume
    );
    assert_eq!(
        watch.look_with_heat(43 * MIN, Ok(()), false, &lease),
        Act::Nothing
    );
    let step = Duration::from_millis(43 * MIN);
    assert!(step > STEP_WITHIN, "the old bound killed it as timeout");
    assert!(step < install::bound_of(&npm), "not timed out");
}

/// #7879: a pause for heat freezes the one-shot and says so in one line,
/// and the resume thaws it with one line.
#[test]
fn the_watch_pauses_openclaws_install_for_heat_with_a_line_each_way() {
    use crate::hermes_schedule::{Act, carry_out_turns};
    let lamp = Lamp::new();
    let records = lamp.records();
    fs::write(
        records.join(RECORD),
        serde_json::json!({"install": "installing", "revision": 1, "at_unix_ms": 1}).to_string(),
    )
    .unwrap();
    let mut systemd = Fake {
        active: Some("activating"),
        ..Fake::default()
    };
    let mut lines = Vec::new();
    let mut say = |line: &str| lines.push(line.to_owned());
    for act in [Act::Pause, Act::Resume] {
        assert_eq!(
            carry_out_turns(&act, &records, &mut systemd, 2, &mut say),
            Carried::Done
        );
    }
    assert_eq!(systemd.asked, ["state", "freeze", "state", "thaw"]);
    assert_eq!(
        lines,
        [
            "OpenClaw's install pauses while the board is hot (board/heat)",
            "OpenClaw's install goes on: the board is no longer hot and the voice is quiet",
        ]
    );
    // The turn passed to Hermes during the pause: the resume says so, not
    // that the board cooled (Heron's P3 on #7942).
    fs::write(
        records.join(hermes_install::RECORD),
        serde_json::json!({"install": "installing", "revision": 1, "at_unix_ms": 1}).to_string(),
    )
    .unwrap();
    lines.clear();
    let mut say = |line: &str| lines.push(line.to_owned());
    assert_eq!(
        carry_out_turns(&Act::Resume, &records, &mut systemd, 3, &mut say),
        Carried::Done
    );
    assert_eq!(
        lines,
        [
            "the pause for a hot board ends: the turn passed to Hermes's install, which heat does not pause"
        ]
    );
}

/// One watch for both: a freeze with OpenClaw's install alone under way is
/// said as OpenClaw's; a stop with both under way records both stopped.
#[test]
fn the_watch_freezes_and_stops_either_install() {
    use crate::hermes_schedule::{Act, carry_out_turns};
    let lamp = Lamp::new();
    let records = lamp.records();
    let installing = |at: u64| {
        serde_json::json!({"install": "installing", "revision": 1, "at_unix_ms": at}).to_string()
    };
    fs::write(records.join(RECORD), installing(1)).unwrap();
    let mut systemd = Fake {
        active: Some("activating"),
        ..Fake::default()
    };
    let mut lines = Vec::new();
    let mut say = |line: &str| lines.push(line.to_owned());
    let freeze = Act::Freeze("someone speaks".into());
    assert_eq!(
        carry_out_turns(&freeze, &records, &mut systemd, 2, &mut say),
        Carried::Done
    );
    fs::write(records.join(hermes_install::RECORD), installing(1)).unwrap();
    let stop = Act::Stop {
        error: "privacy_lost",
        why: "the lease is gone".into(),
    };
    assert_eq!(
        carry_out_turns(&stop, &records, &mut systemd, 3, &mut say),
        Carried::Stopped
    );
    assert_eq!(systemd.asked, ["state", "freeze", "state", "thaw", "stop"]);
    assert_eq!(lamp.said().unwrap().error.as_deref(), Some("privacy_lost"));
    assert_eq!(
        hermes_install::record(&records).unwrap().error.as_deref(),
        Some("privacy_lost")
    );
    assert!(
        lines[0].starts_with("OpenClaw's install is frozen"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Hermes's install is stopped"))
    );
    assert!(
        lines.iter().any(
            |line| line.starts_with("OpenClaw's install, in the same one-shot, is stopped too")
        )
    );
}

/// (#6226) OpenClaw's config, written by hand on lamp-c0a4: the install
/// sets only its `gateway` block (local, loopback, 18789, the token made
/// once) and keeps every key lamp-tasks merged in; a token already there
/// is kept; a config not read as a JSON object is left as it is.
#[test]
fn the_gateway_block_is_set_in_a_config_that_lacks_it_and_other_keys_are_kept() {
    let lamp = Lamp::new();
    fs::create_dir_all(lamp.at(STATE)).unwrap();
    let theirs = serde_json::json!({
        "models": {"mode": "merge", "providers": {"lampos": {
            "baseUrl": "http://127.0.0.1:7741/v1/deepseek",
            "apiKey": "${LAMP_HERMES_TOKEN}",
            "api": "openai-completions",
        }}},
        "agents": {"defaults": {"model": {"primary": "lampos/deepseek-flash"}}},
        "plugins": {"allow": ["openai", "memory-core"]},
        "unknown": [1, 2],
    });
    fs::write(lamp.at(CONFIG), theirs.to_string()).unwrap();
    fs::write(lamp.at(GATEWAY_TOKEN), "kept0123\n").unwrap();
    let runs = Runs::new(&lamp);
    gateway_config(lamp.root(), &mut runs.runner(), &mut quiet()).unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.at(CONFIG)).unwrap()).unwrap();
    for key in ["models", "agents", "plugins", "unknown"] {
        assert_eq!(config[key], theirs[key], "{key} kept");
    }
    assert_eq!(
        config["gateway"],
        serde_json::json!({
            "mode": "local",
            "bind": "loopback",
            "port": 18789,
            "auth": {"mode": "token", "token": "kept0123"},
        })
    );
    assert_eq!(
        fs::read_to_string(lamp.at(GATEWAY_TOKEN)).unwrap(),
        "kept0123\n",
        "the token is made once"
    );
    assert!(
        !runs.lines().iter().any(|line| line.contains(GATEWAY_TOKEN)),
        "a kept token is not given again"
    );
    runs.position(&format!("chown -h lampos:lampos {CONFIG}"));

    // A config lamp-update cannot read as an object: left as it is.
    fs::write(lamp.at(CONFIG), "{ json5: 'not json', }").unwrap();
    gateway_config(lamp.root(), &mut runs.runner(), &mut quiet()).unwrap();
    assert_eq!(
        fs::read_to_string(lamp.at(CONFIG)).unwrap(),
        "{ json5: 'not json', }"
    );
}

/// #7868: the install's `gateway` block is checked by OpenClaw's own
/// `config validate --json` before it lands, as `lampos` in OpenClaw's
/// namespace, on a file beside the config, never the live one. Refused,
/// the install fails with OpenClaw's words, the config stays byte for
/// byte and nothing is left beside it; taken, the block is in place.
#[test]
fn a_gateway_block_openclaws_own_check_refuses_never_lands() {
    let lamp = Lamp::new();
    fs::create_dir_all(lamp.at(STATE)).unwrap();
    fs::write(lamp.at(NS), "").unwrap();
    let theirs = r#"{"models": {"mode": "merge"}}"#;
    fs::write(lamp.at(CONFIG), theirs).unwrap();
    let refusing = Runs {
        fail_on: Some("config validate --json"),
        error: "models.mode: Unrecognized key",
        ..Runs::new(&lamp)
    };
    let error = gateway_config(lamp.root(), &mut refusing.runner(), &mut quiet()).unwrap_err();
    assert!(error.contains("Unrecognized key"), "{error}");
    assert_eq!(fs::read_to_string(lamp.at(CONFIG)).unwrap(), theirs);
    assert_eq!(fs::read_dir(lamp.at(STATE)).unwrap().count(), 1);
    let check = refusing
        .lines()
        .into_iter()
        .find(|line| line.contains("config validate --json"))
        .unwrap();
    let (uid, gid) = own_ids();
    assert!(
        check.starts_with(&format!(
            "{AS_LAMPOS}LAMP_NS_UID={uid} LAMP_NS_GID={gid} OPENCLAW_CONFIG_PATH={STATE}/.openclaw.json."
        )) && check.ends_with(&format!(
            ".new LAMP_HERMES_TOKEN=lamp-config-check /usr/bin/unshare --map-root-user --mount --pid --fork --kill-child --mount-proc --propagation private {NS} config validate --json"
        )),
        "{check}"
    );

    let taking = Runs::new(&lamp);
    gateway_config(lamp.root(), &mut taking.runner(), &mut quiet()).unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.at(CONFIG)).unwrap()).unwrap();
    assert_eq!(config["models"], serde_json::json!({"mode": "merge"}));
    assert_eq!(config["gateway"]["port"], 18789);
    assert_eq!(fs::read_dir(lamp.at(STATE)).unwrap().count(), 1);
    assert!(
        taking.position("config validate --json")
            < taking.position(&format!("chown -h lampos:lampos {CONFIG}"))
    );
}

/// Whether `folder`'s lock (an exclusive `flock` on the folder) is held
/// by another writer just now.
fn folder_lock_held(folder: &Path) -> bool {
    use rustix::fs::{FlockOperation, Mode, OFlags};
    let dir = rustix::fs::open(
        folder,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .unwrap();
    match rustix::fs::flock(&dir, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => false,
        Err(rustix::io::Errno::WOULDBLOCK) => true,
        Err(error) => panic!("flock: {error}"),
    }
}

/// Seventh's P2 on #7872: `openclaw.json` has two read-merge-check-rename
/// writers (this one and lamp-tasks' provider merge), and a rename landing
/// inside the other's check drops the other's block. So the `gateway`
/// block's write holds the state folder's lock from before its read
/// through OpenClaw's own check to the rename, and lets it go after.
#[test]
fn a_gateway_block_write_holds_the_state_folders_lock_through_its_check() {
    let lamp = Lamp::new();
    fs::create_dir_all(lamp.at(STATE)).unwrap();
    fs::write(lamp.at(NS), "").unwrap();
    let runs = Runs::new(&lamp);
    let mut inner = runs.runner();
    let mut held = None;
    let mut runner = |argv: &[String]| {
        if argv.join(" ").contains("config validate --json") {
            held = Some(folder_lock_held(&lamp.at(STATE)));
        }
        inner(argv)
    };
    assert!(gateway_config(lamp.root(), &mut runner, &mut quiet()).unwrap());
    assert_eq!(
        held,
        Some(true),
        "the state folder was not locked through the check"
    );
    assert!(
        !folder_lock_held(&lamp.at(STATE)),
        "the lock outlived the write"
    );
}

/// #8060: lamp-tasks, already `lampos`, runs the same own-namespace check
/// as the `gateway` block with no gateway, only without `runuser`; and a
/// `MainPID` of 0, or none printed, is no gateway running.
#[test]
fn lamp_tasks_own_namespace_check_is_the_installs_without_runuser() {
    let all = validate_argv("1001", "1002", "/x/.openclaw.json.new", false);
    let own = own_namespace_validate_argv("1001", "1002", "/x/.openclaw.json.new");
    assert_eq!(all[..4], ["runuser", "-u", USER, "--"]);
    assert_eq!(own, all[4..]);
    assert_eq!(own[0], "env");
    assert!(own.contains(&CHECK_TOKEN.to_owned()), "{own:?}");
    assert!(own.contains(&NS.to_owned()), "{own:?}");
    assert_eq!(main_pid_argv()[0], "/usr/bin/systemctl");
    assert_eq!(main_pid_argv()[1..], MAIN_PID[1..]);
    let says = |pid: &str| gateway_is_running(&["echo".to_owned(), pid.to_owned()]);
    assert!(says("4242") && !says("0") && !says(""));
    assert!(!gateway_is_running(&["false".to_owned()]) && !gateway_is_running(&[]));
}

/// #7868 (the first builder, 02:02): OpenClaw 2026.11.2 refuses a `config
/// validate` run in namespaces other than its running gateway's
/// (`GatewayLockNamespaceError`, #7268). With the gateway's unit holding a
/// `MainPID`, the check runs as `lampos` through [`WRAPPER`], which joins
/// that gateway (never `LAMP_OPENCLAW_GATEWAY`, no stand-in token: the
/// wrapper passes the forwarder's own); with none running (`0`, as at a
/// first install), in namespaces of its own as before, since there is no
/// lock to clash with.
#[test]
fn a_gateway_block_check_joins_the_running_gateway_and_runs_alone_with_none() {
    let lamp = Lamp::new();
    fs::create_dir_all(lamp.at(STATE)).unwrap();
    fs::write(lamp.at(NS), "").unwrap();
    let check_of = |main_pid: &'static str| {
        let _ = fs::remove_file(lamp.at(CONFIG));
        let runs = Runs {
            main_pid,
            ..Runs::new(&lamp)
        };
        gateway_config(lamp.root(), &mut runs.runner(), &mut quiet()).unwrap();
        let lines = runs.lines();
        let check = lines
            .iter()
            .position(|line| line.contains("config validate --json"))
            .unwrap();
        assert_eq!(
            lines[..check]
                .iter()
                .filter(|line| line.contains("MainPID"))
                .count(),
            1,
            "{lines:#?}"
        );
        lines[check].clone()
    };
    let joined = check_of("4242");
    let (prefix, suffix) = joined
        .split_once(".new ")
        .unwrap_or_else(|| panic!("{joined}"));
    assert!(
        prefix.starts_with(&format!(
            "{AS_LAMPOS}OPENCLAW_CONFIG_PATH={STATE}/.openclaw.json."
        )),
        "{joined}"
    );
    assert_eq!(suffix, format!("{WRAPPER} config validate --json"));
    assert!(!joined.contains("LAMP_OPENCLAW_GATEWAY"), "{joined}");
    assert!(!joined.contains("LAMP_HERMES_TOKEN"), "{joined}");

    let alone = check_of("0");
    assert!(
        alone.ends_with(&format!(
            ".new LAMP_HERMES_TOKEN=lamp-config-check /usr/bin/unshare --map-root-user --mount --pid --fork --kill-child --mount-proc --propagation private {NS} config validate --json"
        )),
        "{alone}"
    );
}

/// (the owner on #6226: every lamp has 6 GB) The gateway is always on with
/// the switch on: once OpenClaw is there the round enables it (the
/// `multi-user.target.wants` link `systemctl enable` makes) and starts it,
/// each round after only starts it when it is down; the switch off
/// disables and stops it.
#[test]
fn the_switch_on_enables_and_starts_the_gateway_and_off_disables_and_stops_it() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    round(&lamp, &mut systemd, true, 1);
    assert!(
        fs::symlink_metadata(lamp.at(WANTS)).is_err(),
        "not enabled before OpenClaw is there"
    );
    assert!(!systemd.asked.contains(&"start-gateway"));
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 2).unwrap().install, "installed");
    runs.position(&format!("systemctl restart {UNIT}"));

    let mut systemd = Fake::default();
    round(&lamp, &mut systemd, true, 3);
    assert_eq!(
        fs::read_link(lamp.at(WANTS)).unwrap(),
        Path::new("/etc/systemd/system/lamp-openclaw-gateway.service")
    );
    assert_eq!(systemd.asked, ["reload", "gateway-state", "start-gateway"]);
    let mut systemd = Fake::default();
    round(&lamp, &mut systemd, true, 4);
    assert_eq!(
        systemd.asked,
        ["gateway-state", "start-gateway"],
        "enabled once"
    );

    let mut systemd = Fake::default();
    after_round_off(lamp.root(), &lamp.records(), &mut systemd, &mut quiet());
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_err(), "disabled");
    assert_eq!(systemd.asked[..2], ["reload", "stop-gateway"]);
}

/// OpenClaw installed (the one-shot run, recorded), then the round after it.
fn installed_openclaw() -> Lamp {
    let lamp = Lamp::new();
    lamp.harness_there();
    round(&lamp, &mut Fake::default(), true, 1);
    assert_eq!(
        install(&lamp, &Runs::new(&lamp), 2).unwrap().install,
        "installed"
    );
    round(&lamp, &mut Fake::default(), true, 3);
    lamp
}

/// A round whose commands `runs` answers, and the lines it said.
fn round_saying(lamp: &Lamp, systemd: &mut Fake, runs: &Runs) -> Vec<String> {
    let mut said = Vec::new();
    after_round(
        lamp.root(),
        &lamp.records(),
        sk(true),
        systemd,
        &mut runs.runner(),
        30,
        Ok(crate::hermes_schedule::Window::Reached),
        &mut |line: &str| said.push(line.to_owned()),
    );
    said
}

/// Harness's one-shot run with this build's own pins over the OpenClaw
/// there, and the lines it said.
fn one_shot_saying(lamp: &Lamp, runs: &Runs) -> Vec<String> {
    let mut said = Vec::new();
    install_with(
        lamp.root(),
        &lamp.records(),
        &mut runs.runner(),
        &mut || 40,
        &mut |line: &str| said.push(line.to_owned()),
        &Kit::BUILT,
    )
    .unwrap();
    said
}

fn config_of(lamp: &Lamp) -> serde_json::Value {
    serde_json::from_slice(&fs::read(lamp.at(CONFIG)).unwrap()).unwrap()
}

/// The `gateway` block the install writes, with the lamp's token.
fn installs_block(lamp: &Lamp) -> serde_json::Value {
    let token = fs::read_to_string(lamp.at(GATEWAY_TOKEN)).unwrap();
    serde_json::json!({
        "mode": "local",
        "bind": "loopback",
        "port": 18789,
        "auth": {"mode": "token", "token": token.trim()},
    })
}

/// A round over a `gateway` block not as the install writes it: it writes
/// nothing of OpenClaw's itself, as `lamp-update.service`'s sandbox never
/// lets it write OpenClaw's state folder (`ProtectSystem=strict`, and a
/// round never changes its own unit, `the_updaters_own_sandbox_is_still_the_7071_images`
/// in main.rs): `openclaw.json` stays byte for byte, no command runs, and
/// Harness's one-shot is asked and started.
fn a_round_asks_the_one_shot(lamp: &Lamp) {
    let before = fs::read(lamp.at(CONFIG)).ok();
    let runs = Runs::new(lamp);
    let mut systemd = Fake::default();
    round_saying(lamp, &mut systemd, &runs);
    assert_eq!(
        fs::read(lamp.at(CONFIG)).ok(),
        before,
        "the round writes it not"
    );
    assert!(
        !runs
            .lines()
            .iter()
            .any(|line| line.contains("config validate") || line.contains("chown")),
        "{:#?}",
        runs.lines()
    );
    assert!(lamp.asked().is_some(), "the one-shot is asked");
    assert!(systemd.asked.contains(&"start"), "and started");
}

/// #7868 (the first builder, 20:11): a reset whose cut was not taken
/// removes `openclaw.json`, and a lamp with OpenClaw installed runs no
/// install, so each round checks the `gateway` block, as #7879 checks the
/// gateway's unit. Removed, the next round asks Harness's one-shot (the
/// round's own sandbox can't write it), which installs nothing: it writes
/// the block again, checked by OpenClaw's own `config validate --json`
/// before the gateway's `try-restart`, every other key kept. The round
/// after it, finding it as written, asks nothing.
#[test]
fn a_gateway_block_removed_with_openclaw_installed_comes_back_at_the_next_round() {
    let lamp = installed_openclaw();
    let mut config = config_of(&lamp);
    let block = config.as_object_mut().unwrap().remove("gateway").unwrap();
    assert_eq!(block, installs_block(&lamp));
    config["models"] = serde_json::json!({"mode": "merge"});
    fs::write(lamp.at(CONFIG), config.to_string()).unwrap();

    a_round_asks_the_one_shot(&lamp);
    let runs = Runs::new(&lamp);
    let said = one_shot_saying(&lamp, &runs);
    let config = config_of(&lamp);
    assert_eq!(config["gateway"], installs_block(&lamp));
    assert_eq!(config["models"], serde_json::json!({"mode": "merge"}));
    assert!(
        runs.position("config validate --json")
            < runs.position(&format!("systemctl try-restart -- {UNIT}")),
        "{:#?}",
        runs.lines()
    );
    assert!(
        !runs
            .lines()
            .iter()
            .any(|line| line.contains("curl") || line.contains(NPM_CI_ENV)),
        "nothing installed: {:#?}",
        runs.lines()
    );
    assert_eq!(lamp.said().unwrap().install, "installed");
    assert!(
        said.iter()
            .any(|line| line.contains("gateway block") && line.contains("written again")),
        "{said:#?}"
    );

    let again = Runs::new(&lamp);
    let mut systemd = Fake::default();
    round_saying(&lamp, &mut systemd, &again);
    assert_eq!(lamp.asked(), None, "nothing asked");
    assert!(!systemd.asked.contains(&"start"), "no one-shot");
}

/// #7868: a `gateway` block not as the install writes it (another port, a
/// changed token) is written again by the one-shot the next round asks, a
/// key of OpenClaw's own in it kept, and the gateway restarted on it.
#[test]
fn a_gateway_block_not_as_the_install_writes_it_is_written_again() {
    let lamp = installed_openclaw();
    let mut config = config_of(&lamp);
    config["gateway"]["port"] = serde_json::json!(1);
    config["gateway"]["auth"]["token"] = serde_json::json!("changed");
    config["gateway"]["controlUi"] = serde_json::json!({"enabled": false});
    fs::write(lamp.at(CONFIG), config.to_string()).unwrap();

    a_round_asks_the_one_shot(&lamp);
    let runs = Runs::new(&lamp);
    one_shot_saying(&lamp, &runs);
    let mut wanted = installs_block(&lamp);
    wanted["controlUi"] = serde_json::json!({"enabled": false});
    assert_eq!(config_of(&lamp)["gateway"], wanted);
    assert!(
        runs.position("config validate --json")
            < runs.position(&format!("systemctl try-restart -- {UNIT}"))
    );
    // #8324: never in the gateway's first start: `settled` looks first.
    assert!(runs.position("ss -Hltn") < runs.position(&format!("systemctl try-restart -- {UNIT}")));
}

/// #7868: OpenClaw's own check refusing the `gateway` block leaves
/// `openclaw.json` byte for byte with nothing beside it, restarts nothing,
/// and says so in one line with no values: OpenClaw's words naming the
/// gateway's token never reach the journal. The next round asks again.
#[test]
fn a_gateway_block_a_round_finds_refused_by_openclaws_check_restarts_nothing() {
    let lamp = installed_openclaw();
    let mut config = config_of(&lamp);
    config.as_object_mut().unwrap().remove("gateway");
    let before = config.to_string();
    fs::write(lamp.at(CONFIG), &before).unwrap();
    let beside = fs::read_dir(lamp.at(STATE)).unwrap().count();
    let token = installs_block(&lamp)["auth"]["token"]
        .as_str()
        .unwrap()
        .to_owned();

    a_round_asks_the_one_shot(&lamp);
    let refusing = Runs {
        fail_on: Some("config validate --json"),
        error: leak(format!("gateway.auth.token: Invalid input \"{token}\"")),
        main_pid: "4242",
        ..Runs::new(&lamp)
    };
    let said = one_shot_saying(&lamp, &refusing);
    assert_eq!(fs::read_to_string(lamp.at(CONFIG)).unwrap(), before);
    assert_eq!(fs::read_dir(lamp.at(STATE)).unwrap().count(), beside);
    assert_eq!(
        refusing
            .lines()
            .iter()
            .filter(|line| line.contains("config validate --json"))
            .count(),
        1,
        "a refusal of the text is never tried again"
    );
    assert!(
        !refusing
            .lines()
            .iter()
            .any(|line| line.contains(&format!("restart -- {UNIT}"))),
        "{:#?}",
        refusing.lines()
    );
    let line = said
        .iter()
        .find(|line| line.contains("gateway block") && line.contains("not written again"))
        .unwrap_or_else(|| panic!("{said:#?}"));
    assert!(!line.contains(&token), "{line}");
    assert!(line.contains("the gateway not restarted"), "{line}");

    a_round_asks_the_one_shot(&lamp);
}

/// The app's tap (#6240, V2's ask of 12 Nov) is a deliberate try again, as
/// for Hermes: a recent failure that waits a day for a round is started at
/// once in the window the tap opens; an install under way is still left
/// alone until it is taken as stuck.
#[test]
fn an_ask_from_the_app_tries_a_recent_openclaw_failure_again() {
    const HOUR_MS: u64 = 60 * 60 * 1000;
    let lamp = Lamp::new();
    lamp.harness_there();
    write(&lamp.records(), failed(100, "no_memory"));
    let mut systemd = Fake::default();
    assert_eq!(round(&lamp, &mut systemd, false, 100 + HOUR_MS), None);
    let asked = |systemd: &mut Fake, at: u64| {
        after_round(
            lamp.root(),
            &lamp.records(),
            sk(false),
            systemd,
            &mut Runs::new(&lamp).runner(),
            at,
            Ok(crate::hermes_schedule::Window::Asked),
            &mut quiet(),
        )
    };
    let mut tapped = Fake::default();
    assert_eq!(
        asked(&mut tapped, 100 + HOUR_MS).unwrap().install,
        "installing"
    );
    // Under way: a tap starts nothing more.
    let mut again = Fake::default();
    assert_eq!(asked(&mut again, 100 + 2 * HOUR_MS), None);
}

/// (#6530) OpenClaw's namespace script and a pane's way into the running
/// gateway's namespaces both put the shared stand-in's folder first on the
/// `PATH`, once, before what runs OpenClaw; the wrapper says once, outside
/// the namespace, that Harness's hooks aren't there yet.
#[test]
fn openclaw_finds_the_hook_stand_in_first_on_its_path() {
    for skills in [false, true] {
        let text = ns_text(sk(skills));
        assert_eq!(text.matches(agent_ns::PATH_LINES).count(), 1);
        let path = text.find(agent_ns::PATH_LINES).unwrap();
        assert!(path < text.rfind(RUN_LINE).unwrap());
        assert!(path < text.find("if [ \"$M\" = on ]; then").unwrap());
    }
    let wrapper = wrapper_text();
    assert_eq!(wrapper.matches(agent_ns::PATH_LINES).count(), 1);
    assert!(wrapper.contains(&format!("{}exec /usr/bin/nsenter ", agent_ns::PATH_LINES)));
    let note = agent_ns::hook_note_line("openclaw");
    assert_eq!(wrapper.matches(&note).count(), 1);
    assert!(wrapper.find(&note) < wrapper.find("LAMP_NS_UID="));
    assert!(wrapper[..512].contains("Written by lamp-update."));
}

/// #6685: OpenClaw takes no skill's run yet (its own fenced path per run is
/// a later step), so its namespace never shows lamp-tasks' runs' token
/// folder, whatever it is asked; [`Shown::tasks`] shows lamp-control's
/// socket alone, read-only (Hermes's own lines), and no round asks for it.
#[test]
fn openclaws_namespace_never_shows_the_runs_token_folder() {
    for skills in [false, true] {
        for tasks in [false, true] {
            let text = ns_text(Shown {
                skills,
                tasks,
                one_path: false,
            });
            assert!(!text.contains("/run/lampos-tasks/runs"), "{text}");
            assert!(!text.to_ascii_lowercase().contains("hermes"), "{text}");
            assert_eq!(text.contains("L=/run/lampos-control/control.sock"), tasks);
        }
        assert_eq!(
            ns_text(Shown {
                skills,
                tasks: false,
                one_path: true,
            }),
            ns_text(sk(skills)),
            "OpenClaw is not on the one run path yet"
        );
    }
}

/// #6685 step 4: with the one run path off, OpenClaw's wrapper is
/// byte for byte as before; on, it is a gateway agent's: the mark and the
/// data after `#!/bin/sh`, the rest as before, lamp-update's mark in its
/// first 512 bytes, and its namespace shows lamp-control's socket alone.
#[test]
fn the_one_path_makes_openclaw_a_gateway_agent_and_off_changes_nothing() {
    for shown in [sk(false), sk(true)] {
        assert_eq!(wrapper_text_for(shown), wrapper_text());
    }
    let on = Shown {
        skills: true,
        tasks: true,
        one_path: true,
    };
    let text = wrapper_text_for(on);
    let head = format!("#!/bin/sh\n{GATEWAY_MARK}{GATEWAY_DATA}");
    assert!(text.starts_with(&head), "{text}");
    assert_eq!(&text[head.len()..], &wrapper_text()["#!/bin/sh\n".len()..]);
    assert!(text[..512].contains(agent_ns::wrapper::MARK));
    for line in GATEWAY_DATA.lines() {
        assert!(line.starts_with("#@"), "{line}");
    }
    assert!(GATEWAY_DATA.contains(&format!("#@port {PORT}\n")));
    assert!(GATEWAY_DATA.contains(&format!("#@ns {NS}\n")));
    assert!(!GATEWAY_DATA.contains("--local"));
    let ns = ns_text(on);
    assert!(ns.contains(
        " L=/run/lampos-control/control.sock B=/run/lampos-tasks/model.sock P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns\nQ=/run/lampos-skill-start\n"
    ));
    assert!(!ns_text(sk(true)).contains("L=/run/lampos-control/control.sock"));
}

/// #6685 (the lead): the one run path alone (`LAMP_SKILL_DEFAULT_AGENT`
/// on, as Hermes's one run path wrapper) shows OpenClaw lamp-control's
/// socket, the pushed skills and the gateway mark: no second switch. Its
/// wrapper and namespace are byte for byte what both switches on wrote
/// before (#6882's `one_path && gateway_runs`, built here the old way), so
/// lamp-tasks' acceptance of that text holds. Off, as before #6685.
#[test]
fn the_one_path_alone_shows_openclaw_what_both_switches_did() {
    for skills in [false, true] {
        let on = shown(skills, true);
        assert!(on.tasks && on.one_path && on.skills == skills);
        // Today's `one_path && gateway_runs` text, built the old way.
        let old = Shown {
            skills,
            tasks: true,
            one_path: true,
        };
        let old_wrapper = format!(
            "#!/bin/sh\n{GATEWAY_MARK}{GATEWAY_DATA}{}",
            wrapper_text().strip_prefix("#!/bin/sh\n").unwrap()
        );
        assert_eq!(wrapper_text_for(on).as_bytes(), old_wrapper.as_bytes());
        assert_eq!(ns_text(on).as_bytes(), ns_text(old).as_bytes());
        let ns = ns_text(on);
        assert!(ns.contains(
            " L=/run/lampos-control/control.sock B=/run/lampos-tasks/model.sock P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns\nQ=/run/lampos-skill-start\n"
        ), "{ns}");
        assert!(ns.contains("/var/lib/lampOS/skills-dev, read-only, when there, for the\n"));
        assert!(ns.contains("if [ -d \"$P\" ]; then mkdir -m 0700"));
        assert!(!ns.contains("if [ \"${LAMP_NS_NET-}\" = run ] && [ -d \"$P\" ]"));
        // The gateway form, never the per-run one: OpenClaw can't open its
        // state outside its gateway's PID namespace.
        let wrapper = wrapper_text_for(on);
        assert!(wrapper.starts_with(&format!("#!/bin/sh\n{GATEWAY_MARK}")));
        assert!(!wrapper.contains(agent_ns::wrapper::ONE_PATH_MARK));
        assert!(wrapper[..512].contains(agent_ns::wrapper::MARK));

        // Off: byte for byte as before #6685.
        let before = sk(skills);
        let off = shown(skills, false);
        assert_eq!(off, before);
        assert_eq!(ns_text(off), ns_text(before));
        assert_eq!(wrapper_text_for(off), wrapper_text());
        assert!(!ns_text(off).contains("/run/lampos-control/control.sock"));
        assert!(!wrapper_text_for(off).contains(GATEWAY_MARK));
    }
}

/// #6685 (the lead): lamp-update reads no gateway runs switch of its own,
/// in any of the files that decide what OpenClaw is shown.
#[test]
fn lamp_update_reads_no_gateway_runs_switch() {
    let switch = concat!("LAMP_SKILL_", "GATEWAY_RUNS");
    for (name, source) in [
        (
            "openclaw_install.rs",
            include_str!("../openclaw_install.rs"),
        ),
        ("switches.rs", include_str!("../switches.rs")),
        ("agent_ns.rs", include_str!("../agent_ns.rs")),
        ("main.rs", include_str!("../main.rs")),
        ("hermes_install.rs", include_str!("../hermes_install.rs")),
    ] {
        assert!(!source.contains(switch), "{name} reads {switch}");
        assert!(!source.contains("GATEWAY_RUNS_SWITCH"), "{name}");
        assert!(!source.contains("gateway_runs"), "{name}");
    }
}

/// #6685 (the lead): nothing the one run path changes writes OpenClaw's
/// shared config ([`CONFIG`]). The one write of it in lamp-update is the
/// install's own `gateway` block ([`gateway_config`], the gateway's token,
/// as before #6685), which takes no [`Shown`]: the switches never reach it.
/// The written wrapper and namespace never name it.
#[test]
fn the_one_path_writes_nothing_to_openclaws_shared_config() {
    let source = include_str!("../openclaw_install.rs");
    let code = source.split("\n#[cfg(test)]").next().unwrap();
    // The writer, and (#7868) a round's read-only check of the `gateway`
    // block, which writes nothing.
    assert_eq!(code.matches("on(root, CONFIG)").count(), 2);
    let reader = code
        .split_once("fn gateway_block_due(")
        .unwrap()
        .1
        .split_once("\n}\n")
        .unwrap()
        .0;
    assert!(reader.contains("fs::read(on(root, CONFIG))"));
    assert!(!reader.contains("write") && !reader.contains("replace"));
    let writer = code
        .split_once("fn gateway_config(")
        .unwrap()
        .1
        .split_once("\n}\n")
        .unwrap()
        .0;
    assert!(writer.contains("on(root, CONFIG)"));
    assert!(!writer.contains("Shown"));
    assert!(
        !code.contains("CONFIG, "),
        "CONFIG handed to another writer"
    );
    for skills in [false, true] {
        for one_path in [false, true] {
            let shown = shown(skills, one_path);
            assert!(!wrapper_text_for(shown).contains("openclaw.json"));
            assert!(!ns_text(shown).contains("openclaw.json"));
        }
    }
    // And the files `shown` reaches are the wrapper and the namespace
    // script alone, never the config.
    let entry_points = code
        .split_once("fn write_entry_points(root: &Path, shown: Shown)")
        .unwrap()
        .1
        .split_once("\n}\n")
        .unwrap()
        .0;
    assert!(entry_points.contains("write_root_file(root, NS, &ns_text(shown), 0o755)"));
    assert!(
        entry_points.contains("write_root_file(root, WRAPPER, &wrapper_text_for(shown), 0o755)")
    );
    assert!(!entry_points.contains("CONFIG"));
}

/// The one run path is carried through the one-shot's ask, so its
/// install writes the wrapper the round looks for.
#[test]
fn the_ask_carries_the_one_path() {
    let dir = tempfile::tempdir().unwrap();
    let on = Shown {
        skills: true,
        tasks: true,
        one_path: true,
    };
    ask(dir.path(), false, on).unwrap();
    let asked: Ask = serde_json::from_slice(&fs::read(dir.path().join(ASK)).unwrap()).unwrap();
    assert!(asked.one_path && asked.tasks && asked.skills);
    ask(dir.path(), false, sk(true)).unwrap();
    let text = fs::read_to_string(dir.path().join(ASK)).unwrap();
    assert!(
        !text.contains("one_path"),
        "off writes the ask as before: {text}"
    );
}

/// #6882 P1-1: with the one run path on, OpenClaw's namespace shows the
/// pushed skills' folder without `LAMP_NS_NET` (the gateway's unit never
/// sets it), each edit found once; off, the text is byte for byte
/// Hermes's with only [`ENGINE_EDITS`], as before.
#[test]
fn the_one_path_shows_the_pushed_skills_without_ns_net_and_off_changes_nothing() {
    for skills in [false, true] {
        for tasks in [false, true] {
            let mut before = hermes_install::ns_text(skills, tasks);
            // A voice make's home lines are Hermes's alone (lamp-voice's
            // `ns_text`): taken out with `tasks`, as there.
            if tasks {
                before = before.replacen(hermes_install::NS_HOME_LINE, "", 1);
            }
            for (from, to) in ENGINE_EDITS {
                before = before.replace(from, to);
            }
            let before = before.replacen(
                RUN_LINE,
                &format!("{NPM_CI_MODE}{GATEWAY_RUN}{RUN_LINE}"),
                1,
            );
            let off = Shown {
                skills,
                tasks,
                one_path: false,
            };
            assert_eq!(ns_text(off), before, "off: as before");
            if !tasks {
                assert_eq!(
                    ns_text(Shown {
                        one_path: true,
                        ..off
                    }),
                    before
                );
                continue;
            }
            for (from, _) in RUNS_EDITS {
                assert_eq!(before.matches(from).count(), 1, "`{from}`");
            }
            let on = ns_text(Shown {
                one_path: true,
                ..off
            });
            assert!(
                on.contains("if [ -d \"$P\" ]; then mkdir -m 0700 \"$S/p\""),
                "{on}"
            );
            // LAMP_NS_NET's other steps stay behind it.
            assert_eq!(
                on.matches("if [ \"${LAMP_NS_NET-}\" = run ]").count(),
                before.matches("if [ \"${LAMP_NS_NET-}\" = run ]").count() - 1
            );
            assert!(on.contains("model.sock"));
            let checked = std::process::Command::new("/bin/sh")
                .args(["-n", "-c", &on])
                .output()
                .unwrap();
            assert!(checked.status.success(), "{checked:?}");
        }
    }
}

/// #6882 P1-1, run under `sh` with every mount stubbed on a stand-in
/// root that has the pushed skills' folder, and no `LAMP_NS_NET` (as the
/// gateway's unit runs it): with the one run path on, the folder is
/// staged, moved back and remounted read-only; off, never touched.
#[test]
fn the_gateways_namespace_shows_the_pushed_skills_read_only_with_the_one_path() {
    for one_path in [false, true] {
        let folder = tempfile::tempdir().unwrap();
        let base = folder.path().to_string_lossy().into_owned();
        let at = |path: &str| format!("{base}/root{path}");
        let stubs = folder.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let log = folder.path().join("log");
        let resolv = at("/run/systemd/resolve/stub-resolv.conf");
        for made in [
            PLACE,
            WORK,
            SKILLS,
            "/etc/lampOS",
            "/dev/shm",
            "/run/systemd/resolve",
            hermes_install::PUSHED,
        ] {
            fs::create_dir_all(at(made)).unwrap();
        }
        fs::write(&resolv, "nameserver 127.0.0.53\n").unwrap();
        let uid_map = folder.path().join("uid_map");
        fs::write(&uid_map, "         0        996          1\n").unwrap();
        let stub = "#!/bin/sh\necho \"${0##*/} $*\" >> \"$LOG\"\n\
                    case ${0##*/} in findmnt) echo rw,nosuid,nodev,relatime;; readlink) echo \"$RESOLV\";; \
                    unshare) echo ran;; esac\n";
        for name in [
            "mount", "findmnt", "mkdir", "rmdir", "rm", "touch", "readlink", "unshare", "ip",
        ] {
            let path = stubs.join(name);
            fs::write(&path, stub).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut text = ns_text(Shown {
            skills: false,
            tasks: true,
            one_path,
        });
        let moved = [
            "/var/tmp",
            "/var/lib",
            "/etc/lampOS",
            "/etc/resolv.conf",
            "/dev/shm",
            "/tmp",
            "/run",
        ];
        for (n, path) in moved.iter().enumerate() {
            text = text.replace(path, &format!("\u{1}{n}\u{1}"));
        }
        for (n, path) in moved.iter().enumerate() {
            text = text.replace(&format!("\u{1}{n}\u{1}"), &at(path));
        }
        let text = text
            .replace("/proc/self/uid_map", &uid_map.to_string_lossy())
            .replace("/usr/bin/unshare", &stubs.join("unshare").to_string_lossy());
        let script = folder.path().join("openclaw-ns");
        fs::write(&script, text).unwrap();
        let out = std::process::Command::new("/bin/sh")
            .arg(&script)
            .arg("gateway")
            .current_dir(at(WORK))
            .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
            .env("LOG", &log)
            .env("RESOLV", &resolv)
            .env("LAMP_NS_UID", "996")
            .env("LAMP_NS_GID", "994")
            .env_remove("LAMP_NS_NET")
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let lines: Vec<String> = fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        let stage = at("/tmp/.lampos-ns");
        let pushed = at(hermes_install::PUSHED);
        let wanted = [
            format!("mkdir -m 0700 {stage}/p"),
            format!("mount --bind {pushed} {stage}/p"),
            format!("mount --move {stage}/p {pushed}"),
            format!("mount -o remount,bind,ro,nosuid,nodev,relatime {pushed}"),
            format!("rmdir {stage}/p"),
        ];
        let places: Vec<Option<usize>> = wanted
            .iter()
            .map(|line| lines.iter().position(|l| l == line))
            .collect();
        if one_path {
            let places: Vec<usize> = places
                .iter()
                .zip(&wanted)
                .map(|(at, line)| at.unwrap_or_else(|| panic!("no `{line}` in {lines:#?}")))
                .collect();
            assert!(places.is_sorted(), "{lines:#?}");
        } else {
            assert!(
                !lines.iter().any(|line| line.contains(&pushed)),
                "{lines:#?}"
            );
        }
        // No model socket and no relay without LAMP_NS_NET, either way.
        assert!(!lines.iter().any(|line| line.contains("model.sock")));
        assert!(!lines.iter().any(|line| line.contains("net-relay")));
    }
}

/// The one run path's launcher and its socket folder (#6685): fixed
/// root paths, never under Harness's project, which every gateway agent
/// can write.
#[test]
fn the_launcher_is_a_fixed_root_path_outside_the_work_folder() {
    assert_eq!(SKILL_START, "/usr/local/lib/lampos/skill-start");
    assert_eq!(START_SOCKET_DIR, "/run/lampos-skill-start");
    // Never under lamp-tasks' runtime folder, which its restart wipes.
    assert!(!START_SOCKET_DIR.starts_with("/run/lampos-tasks"));
    assert_eq!(TASKS_BINARY, "/opt/lampOS/tasks/current/lamp-tasks");
    assert_eq!(Path::new(SKILL_START).parent(), Path::new(NS).parent());
    for path in [SKILL_START, START_SOCKET_DIR] {
        assert!(!ALLOWED.iter().any(|shown| path.starts_with(shown)));
    }
}

fn one_path() -> Shown {
    shown(true, true)
}

const TASKS_BYTES: &[u8] = b"\x7fELF a stand-in for lamp-tasks";

/// The installed lamp-tasks, with `bytes`.
fn tasks_there(lamp: &Lamp, bytes: &[u8]) {
    let at = lamp.at(TASKS_BINARY);
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    fs::write(at, bytes).unwrap();
}

/// The approvals lines of `runs`.
fn approvals(runs: &Runs) -> Vec<String> {
    runs.lines()
        .into_iter()
        .filter(|line| line.contains(" approvals "))
        .collect()
}

/// An allow-list edit as lamp-update runs it: as `lampos`, through the
/// wrapper, into the running gateway.
fn approvals_line(edit: &str, agent: &str, path: &str) -> String {
    format!("{AS_LAMPOS}{WRAPPER} approvals allowlist {edit} --gateway --agent {agent} {path}")
}

fn on_lines() -> [String; 3] {
    [
        approvals_line("add", "main", SKILL_START),
        approvals_line("remove", "main", OLD_RUN_START),
        approvals_line("remove", "lamp-skill-run", OLD_RUN_START),
    ]
}

/// An install with this build's own pins, as the one-shot runs it over
/// an OpenClaw already there.
fn install_built(lamp: &Lamp, runs: &Runs, at: u64) -> Option<OpenclawInstall> {
    let mut run = runs.runner();
    install_with(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || at,
        &mut quiet(),
        &Kit::BUILT,
    )
    .unwrap()
}

fn round_shown(lamp: &Lamp, systemd: &mut Fake, shown: Shown) -> Option<OpenclawInstall> {
    round_with(lamp, systemd, shown, &Runs::new(lamp))
}

/// A round whose commands `runs` answers and logs.
fn round_with(
    lamp: &Lamp,
    systemd: &mut Fake,
    shown: Shown,
    runs: &Runs,
) -> Option<OpenclawInstall> {
    after_round(
        lamp.root(),
        &lamp.records(),
        shown,
        systemd,
        &mut runs.runner(),
        10,
        Ok(crate::hermes_schedule::Window::Reached),
        &mut quiet(),
    )
}

/// #6685: with the one run path on, the install writes the launcher (a
/// byte copy of lamp-tasks, 0755), restarts the gateway, then gives agent
/// `main` the launcher's exact path through the running gateway as
/// `lampos` and takes the earlier glob out of `main` and the withdrawn
/// fence agent; never through OpenClaw's config.
#[test]
fn the_one_path_lets_agent_main_run_the_launcher_alone() {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    assert!(lamp.asked().unwrap().one_path);
    let runs = Runs::new(&lamp);
    let said = install(&lamp, &runs, 20).unwrap();
    assert_eq!(said.install, "installed", "{:#?}", runs.lines());
    assert!(said.exec_allowlist);
    assert_eq!(fs::read(lamp.at(SKILL_START)).unwrap(), TASKS_BYTES);
    assert!(
        fs::symlink_metadata(lamp.at(SKILL_START))
            .unwrap()
            .is_file()
    );
    assert_eq!(mode(&lamp.at(SKILL_START)), 0o755);
    assert_eq!(approvals(&runs), on_lines());
    assert_eq!(
        fs::read_to_string(lamp.unit()).unwrap(),
        unit_text_for(one_path())
    );
    let made = runs.position(&format!(
        "install -d -o lampos -g lampos -m 0755 {START_SOCKET_DIR}"
    ));
    let restart = runs.position(&format!("systemctl restart {UNIT}"));
    assert!(
        made < restart
            && runs.position("systemctl try-restart lamp-tasks.service") < restart
            // The allow-list's look (#8324: `settled` looks before it too).
            && runs.lines()[restart..].contains(&format!("systemctl is-active --quiet {UNIT}"))
            && restart < runs.position(" approvals allowlist add ")
    );
    assert!(
        !approvals(&runs)
            .iter()
            .any(|line| line.contains("openclaw.json")
                || line.contains("approvals set")
                || line.contains(NS)
                || line.contains('*') && line.contains(" add "))
    );
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(lamp.at(CONFIG)).unwrap()).unwrap();
    assert_eq!(
        config.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["gateway"],
        "the install's own key alone: the one path writes none"
    );

    // Done: a round asks nothing, and an install asked again runs no
    // approvals command and leaves the launcher as it is.
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    assert_eq!(lamp.asked(), None);
    let written = fs::metadata(lamp.at(SKILL_START)).unwrap().ino();
    ask(&lamp.records(), false, one_path()).unwrap();
    let again = Runs::new(&lamp);
    assert!(install_built(&lamp, &again, 30).unwrap().exec_allowlist);
    assert_eq!(approvals(&again), Vec::<String>::new());
    assert_eq!(fs::metadata(lamp.at(SKILL_START)).unwrap().ino(), written);
}

/// A new lamp-tasks: the round sees the launcher is not its copy and asks
/// the one-shot, which writes it again without installing anything.
#[test]
fn the_launcher_follows_the_installed_lamp_tasks() {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    install(&lamp, &Runs::new(&lamp), 20).unwrap();
    round_shown(&lamp, &mut systemd, one_path());
    assert_eq!(lamp.asked(), None);

    tasks_there(&lamp, b"\x7fELF the next lamp-tasks");
    round_shown(&lamp, &mut systemd, one_path());
    assert!(lamp.asked().unwrap().one_path, "asked again");
    let runs = Runs::new(&lamp);
    let said = install_built(&lamp, &runs, 40).unwrap();
    assert!(said.install == "installed" && said.exec_allowlist);
    assert_eq!(
        fs::read(lamp.at(SKILL_START)).unwrap(),
        b"\x7fELF the next lamp-tasks"
    );
    assert_eq!(mode(&lamp.at(SKILL_START)), 0o755);
    assert!(
        !runs
            .lines()
            .iter()
            .any(|line| line.contains(NPM_CI_ENV) || line.contains("systemctl restart"))
    );

    // A mode not 0755 is due too.
    fs::set_permissions(lamp.at(SKILL_START), fs::Permissions::from_mode(0o775)).unwrap();
    assert!(launcher_due(lamp.root(), one_path()));
}

/// An OpenClaw installed by an earlier build (the glob's, or none): the
/// round asks the one-shot, which writes the launcher and edits the
/// allow-list without installing anything again.
#[test]
fn an_openclaw_installed_before_the_launcher_gets_it() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    install(&lamp, &Runs::new(&lamp), 20).unwrap();
    let mut old = lamp.said().unwrap();
    old.exec_allowlist = false;
    write(&lamp.records(), old);
    tasks_there(&lamp, TASKS_BYTES);

    round_shown(&lamp, &mut systemd, one_path());
    assert!(lamp.asked().unwrap().one_path, "asked again");
    let runs = Runs::new(&lamp);
    let said = install_built(&lamp, &runs, 40).unwrap();
    assert!(said.install == "installed" && said.exec_allowlist);
    assert_eq!(fs::read(lamp.at(SKILL_START)).unwrap(), TASKS_BYTES);
    assert_eq!(approvals(&runs), on_lines());
    assert!(!runs.lines().iter().any(|line| line.contains(NPM_CI_ENV)));
}

/// An add that fails (the gateway not answering yet) is not done: the
/// install is still `installed`, the entry points stay, the record says
/// no entry, and the next round asks again; the next try adds it.
#[test]
fn a_failed_add_is_retried_and_never_fails_the_install() {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    let runs = Runs {
        fail_on: Some(" approvals allowlist add "),
        ..Runs::new(&lamp)
    };
    let said = install(&lamp, &runs, 20).unwrap();
    assert_eq!(
        (said.install.as_str(), said.error.as_deref()),
        ("installed", None)
    );
    assert!(!said.exec_allowlist);
    assert!(lamp.at(WRAPPER).exists() && lamp.at(NS).exists() && lamp.at(SKILL_START).exists());
    // #8444: the failed add runs once more after the gateway settled.
    let [add, main, fence] = on_lines();
    assert_eq!(
        approvals(&runs),
        [add.clone(), add, main, fence],
        "the removes still run"
    );

    round_shown(&lamp, &mut systemd, one_path());
    assert!(lamp.asked().unwrap().one_path, "asked again");
    let runs = Runs::new(&lamp);
    assert!(install_built(&lamp, &runs, 30).unwrap().exec_allowlist);
    assert_eq!(
        approvals(&runs)[0],
        approvals_line("add", "main", SKILL_START)
    );
}

/// The gateway not running: nothing is run against it, said, and retried.
#[test]
fn no_gateway_running_leaves_the_allow_list_for_the_next_round() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    install(&lamp, &Runs::new(&lamp), 20).unwrap();
    let mut old = lamp.said().unwrap();
    old.exec_allowlist = false;
    write(&lamp.records(), old);
    ask(&lamp.records(), false, one_path()).unwrap();
    let runs = Runs {
        fail_on: Some(" is-active "),
        ..Runs::new(&lamp)
    };
    let said = install_built(&lamp, &runs, 30).unwrap();
    assert!(said.install == "installed" && !said.exec_allowlist);
    assert_eq!(approvals(&runs), Vec::<String>::new());
}

/// A remove that fails is passed over: the add still counts.
#[test]
fn a_failed_remove_is_not_fatal() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    let runs = Runs {
        fail_on: Some(" approvals allowlist remove "),
        ..Runs::new(&lamp)
    };
    let said = install(&lamp, &runs, 20).unwrap();
    assert!(said.install == "installed" && said.exec_allowlist);
    // #8444: main's failed remove runs once more; the old fence agent's,
    // expected to fail, does not.
    let [add, main, fence] = on_lines();
    assert_eq!(approvals(&runs), [add, main.clone(), main, fence]);
}

/// With the one run path off, the install takes the launcher and agent
/// `main`'s entries out (failures passed over), and adds nothing.
#[test]
fn the_one_path_off_takes_the_launcher_and_the_entries_out() {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    install(&lamp, &Runs::new(&lamp), 20).unwrap();
    assert!(lamp.at(SKILL_START).exists());
    fs::write(lamp.at(START_SOCKET_DIR).join("start.sock"), "").unwrap();

    // Off: the round sees the entry recorded and the launcher there.
    round_shown(&lamp, &mut systemd, sk(true));
    assert!(!lamp.asked().unwrap().one_path, "asked with it off");
    let runs = Runs {
        fail_on: Some(" approvals allowlist remove "),
        ..Runs::new(&lamp)
    };
    let said = install(&lamp, &runs, 30).unwrap();
    assert!(said.install == "installed" && !said.exec_allowlist);
    assert!(fs::symlink_metadata(lamp.at(SKILL_START)).is_err());
    assert!(fs::symlink_metadata(lamp.at(START_SOCKET_DIR)).is_err());
    assert_eq!(fs::read_to_string(lamp.unit()).unwrap(), UNIT_TEXT);
    assert!(
        !runs
            .lines()
            .iter()
            .any(|line| line.contains(START_SOCKET_DIR))
    );
    assert_eq!(
        approvals(&runs),
        // #8444: each failed remove runs once more.
        [
            approvals_line("remove", "main", SKILL_START),
            approvals_line("remove", "main", SKILL_START),
            approvals_line("remove", "main", OLD_RUN_START),
            approvals_line("remove", "main", OLD_RUN_START)
        ]
    );
    assert!(!launcher_due(lamp.root(), sk(true)));
    round_shown(&lamp, &mut systemd, sk(true));
    assert_eq!(lamp.asked(), None, "nothing left to do");
}

/// #6685: a factory reset wipes OpenClaw's approvals and keeps the
/// record (`exec_allowlist` true): every round with the gateway up runs
/// agent `main`'s add again (only it: no remove, no install, no one-shot).
/// An add refused marks the record and asks the one-shot; with the
/// gateway down the round runs nothing and keeps the record.
#[test]
fn every_round_adds_agent_mains_entry_again_with_the_gateway_up() {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    install(&lamp, &Runs::new(&lamp), 20).unwrap();
    assert!(lamp.said().unwrap().exec_allowlist);

    for _ in 0..2 {
        let runs = Runs::new(&lamp);
        assert_eq!(round_with(&lamp, &mut systemd, one_path(), &runs), None);
        assert_eq!(
            approvals(&runs),
            [approvals_line("add", "main", SKILL_START)]
        );
        assert!(
            runs.position(&format!("systemctl is-active --quiet {UNIT}"))
                < runs.position(" approvals ")
        );
        assert_eq!(lamp.asked(), None, "no one-shot");
        assert!(lamp.said().unwrap().exec_allowlist);
    }

    let down = Runs {
        fail_on: Some(" is-active "),
        ..Runs::new(&lamp)
    };
    round_with(&lamp, &mut systemd, one_path(), &down);
    assert_eq!(approvals(&down), Vec::<String>::new());
    assert!(lamp.said().unwrap().exec_allowlist && lamp.asked().is_none());

    let refused = Runs {
        fail_on: Some(" approvals allowlist add "),
        ..Runs::new(&lamp)
    };
    round_with(&lamp, &mut systemd, one_path(), &refused);
    assert!(!lamp.said().unwrap().exec_allowlist);
    assert!(lamp.asked().unwrap().one_path, "the one-shot is asked");
    let runs = Runs::new(&lamp);
    assert!(install_built(&lamp, &runs, 40).unwrap().exec_allowlist);
    assert_eq!(approvals(&runs), on_lines());

    // With the one run path off, a round adds nothing.
    let off = Runs::new(&lamp);
    round_with(&lamp, &mut systemd, sk(true), &off);
    assert!(!approvals(&off).iter().any(|line| line.contains(" add ")));
}

/// #7135 (c0a4's reboot #3, 22:59:00): a round right after a boot found
/// the gateway active but not ready ("Config unavailable"), read it as the
/// entry gone, and asked the one-shot, which reinstalled OpenClaw. Not
/// ready is unknown: the add is asked again until taken; never taken, the
/// record keeps the entry and nothing is asked. Another refusal still
/// marks the record and asks the one-shot, once.
#[test]
fn a_round_reads_a_gateway_not_ready_as_unknown_never_as_the_entry_gone() {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    install(&lamp, &Runs::new(&lamp), 20).unwrap();
    assert!(lamp.said().unwrap().exec_allowlist && lamp.asked().is_none());

    let round = |systemd: &mut Fake, run: &mut Runner| {
        after_round(
            lamp.root(),
            &lamp.records(),
            one_path(),
            systemd,
            run,
            30,
            Ok(crate::hermes_schedule::Window::Reached),
            &mut quiet(),
        )
    };
    let not_ready = [
        format!("{CONFIG_NOT_READY}; cannot validate --agent."),
        format!("/usr/bin/env: {NODE_NOT_READY}"),
    ];
    for why in &not_ready {
        // Not ready twice, then taken: one round, the entry kept.
        let mut adds = 0;
        let mut run = |argv: &[String]| -> Result<String, String> {
            if argv.iter().any(|arg| arg == "add") {
                adds += 1;
                if adds < 3 {
                    return Err(why.clone());
                }
            }
            Ok(String::new())
        };
        round(&mut systemd, &mut run);
        assert_eq!(adds, 3, "{why}");
        assert!(lamp.said().unwrap().exec_allowlist, "{why}");
        assert_eq!(lamp.asked(), None, "{why}");

        // Never ready in the round: unknown, nothing marked or asked.
        let mut adds = 0;
        let mut run = |argv: &[String]| -> Result<String, String> {
            if argv.iter().any(|arg| arg == "add") {
                adds += 1;
                return Err(why.clone());
            }
            Ok(String::new())
        };
        round(&mut systemd, &mut run);
        assert_eq!(adds, ADD_TRIES, "{why}");
        assert!(lamp.said().unwrap().exec_allowlist, "{why}");
        assert_eq!(lamp.asked(), None, "no reinstall: {why}");
    }

    // Another refusal: marked and asked, not tried again.
    let mut adds = 0;
    let mut run = |argv: &[String]| -> Result<String, String> {
        if argv.iter().any(|arg| arg == "add") {
            adds += 1;
            return Err("unknown agent".into());
        }
        Ok(String::new())
    };
    round(&mut systemd, &mut run);
    assert_eq!(adds, 1);
    assert!(!lamp.said().unwrap().exec_allowlist);
    assert!(lamp.asked().unwrap().one_path, "the one-shot is asked");
}

/// The one run path switched off with the gateway down: nothing is run,
/// and the record keeps the entry, so the next round asks again; with
/// the gateway up, the removes run and the record drops it.
#[test]
fn a_switch_off_with_the_gateway_down_is_retried_until_the_removes_run() {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    install(&lamp, &Runs::new(&lamp), 20).unwrap();

    round_shown(&lamp, &mut systemd, sk(true));
    assert!(!lamp.asked().unwrap().one_path);
    let down = Runs {
        fail_on: Some(" is-active "),
        ..Runs::new(&lamp)
    };
    let said = install_built(&lamp, &down, 30).unwrap();
    assert!(said.install == "installed" && said.exec_allowlist);
    assert_eq!(approvals(&down), Vec::<String>::new());

    // Again with the gateway down, OpenClaw now there as off wants it
    // (nothing installed): still kept.
    round_shown(&lamp, &mut systemd, sk(true));
    assert!(present(lamp.root(), sk(true)) && !lamp.asked().unwrap().one_path);
    let down = Runs {
        fail_on: Some(" is-active "),
        ..Runs::new(&lamp)
    };
    let said = install_built(&lamp, &down, 35).unwrap();
    assert!(said.install == "installed" && said.exec_allowlist);
    assert!(!down.lines().iter().any(|line| line.contains(NPM_CI_ENV)));

    round_shown(&lamp, &mut systemd, sk(true));
    assert!(!lamp.asked().unwrap().one_path, "asked again");
    let up = Runs::new(&lamp);
    let said = install_built(&lamp, &up, 40).unwrap();
    assert!(said.install == "installed" && !said.exec_allowlist);
    assert_eq!(
        approvals(&up),
        [
            approvals_line("remove", "main", SKILL_START),
            approvals_line("remove", "main", OLD_RUN_START)
        ]
    );
    round_shown(&lamp, &mut systemd, sk(true));
    assert_eq!(lamp.asked(), None, "nothing left to do");
}

/// OpenClaw off on the lamp: the launcher and the socket folder go with
/// the entry points.
#[test]
fn openclaw_off_takes_the_launcher_out() {
    let lamp = Lamp::new();
    fs::write(lamp.at(SKILL_START), TASKS_BYTES).unwrap();
    fs::write(lamp.at(WRAPPER), "old").unwrap();
    fs::create_dir_all(lamp.at(START_SOCKET_DIR)).unwrap();
    ask(&lamp.records(), true, Shown::default()).unwrap();
    assert_eq!(install(&lamp, &Runs::new(&lamp), 9), None);
    assert!(fs::symlink_metadata(lamp.at(SKILL_START)).is_err());
    assert!(fs::symlink_metadata(lamp.at(START_SOCKET_DIR)).is_err());
}

/// The exact edits, on and off.
#[test]
fn the_allow_list_edits_name_the_launcher_exactly() {
    assert_eq!(
        exec_allowlist_args(one_path())[0],
        [
            "approvals",
            "allowlist",
            "add",
            "--gateway",
            "--agent",
            "main",
            SKILL_START
        ]
    );
    for shown in [one_path(), sk(true)] {
        for args in exec_allowlist_args(shown) {
            assert_eq!(args[3], "--gateway");
            assert!(args[2] == "remove" || args[6] == SKILL_START);
        }
    }
    assert!(
        exec_allowlist_args(sk(true))
            .iter()
            .all(|args| args[2] == "remove")
    );
}

/// #6685: with the one run path on, the gateway's unit makes the start
/// socket folder at each start and keeps it past a stop; off, the unit is
/// as before, byte for byte.
#[test]
fn the_gateway_unit_keeps_the_start_socket_folder_with_the_one_path() {
    assert_eq!(unit_text_for(sk(true)), UNIT_TEXT);
    assert_eq!(unit_text_for(Shown::default()), UNIT_TEXT);
    let on = unit_text_for(one_path());
    assert_eq!(
        on.replacen(START_DIR_LINES, "", 1).replace(
            CONTROL_LINES,
            "After=network-online.target lamp-cloud.service\n"
        ),
        UNIT_TEXT
    );
    for line in [
        "RuntimeDirectory=lampos-skill-start",
        "RuntimeDirectoryMode=0755",
        "RuntimeDirectoryPreserve=yes",
    ] {
        assert_eq!(on.lines().filter(|l| *l == line).count(), 1, "{line}");
    }
    assert_eq!(format!("/run/{}", "lampos-skill-start"), START_SOCKET_DIR);
    let service = on.find("[Service]").unwrap();
    assert!(on.find("RuntimeDirectory=").unwrap() > service);
    assert!(on.find("RuntimeDirectory=").unwrap() < on.find("ExecStart=").unwrap());
}

/// #6685: with the one run path on, the gateway's namespace shows the
/// start socket folder read-only, skipped when it is not there; off,
/// never touched.
#[test]
fn the_gateways_namespace_shows_the_start_socket_folder_read_only() {
    for (one_path, folder_there) in [(false, true), (true, true), (true, false)] {
        let lines = run_ns_stubbed(one_path, |at| {
            if folder_there {
                fs::create_dir_all(at(START_SOCKET_DIR)).unwrap();
            }
        });
        let (stage, start) = lines.1;
        let lines = lines.0;
        let wanted = [
            format!("mkdir -m 0700 {stage}/q"),
            format!("mount --bind {start} {stage}/q"),
            format!("mount --move {stage}/q {start}"),
            format!("mount -o remount,bind,ro,nosuid,nodev,relatime {start}"),
            format!("rmdir {stage}/q"),
        ];
        if one_path && folder_there {
            let places: Vec<usize> = wanted
                .iter()
                .map(|line| {
                    lines
                        .iter()
                        .position(|l| l == line)
                        .unwrap_or_else(|| panic!("no `{line}` in {lines:#?}"))
                })
                .collect();
            assert!(places.is_sorted(), "{lines:#?}");
        } else {
            assert!(
                !lines.iter().any(|line| line.contains("lampos-skill-start")),
                "{lines:#?}"
            );
        }
    }
    let on = ns_text(one_path());
    for line in [
        "\nQ=/run/lampos-skill-start\n",
        "if [ -d \"$Q\" ]; then mkdir -m 0700 \"$S/q\" && mount --bind \"$Q\" \"$S/q\" || fail \"no $Q\"; else Q=; fi\n",
        "if [ -n \"$Q\" ]; then mkdir -p \"$Q\" && mount --move \"$S/q\" \"$Q\" && ro \"$Q\" || fail \"$Q not shown\"; fi\n",
    ] {
        assert_eq!(on.matches(line).count(), 1, "{line}");
    }
    for shown in [sk(false), sk(true)] {
        assert!(!ns_text(shown).contains("lampos-skill-start"));
    }
}

/// [`ns_text`] for `one_path` (tasks on, skills off) run under `sh` with
/// every mount stubbed on a stand-in root `made` fills, no `LAMP_NS_NET`
/// (as the gateway's unit runs it): the stubs' log, and the staging folder
/// and [`START_SOCKET_DIR`] as the script names them.
fn run_ns_stubbed(
    one_path: bool,
    made: impl Fn(&dyn Fn(&str) -> String),
) -> (Vec<String>, (String, String)) {
    let folder = tempfile::tempdir().unwrap();
    let base = folder.path().to_string_lossy().into_owned();
    let at = |path: &str| format!("{base}/root{path}");
    let stubs = folder.path().join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let log = folder.path().join("log");
    let resolv = at("/run/systemd/resolve/stub-resolv.conf");
    for dir in [
        PLACE,
        WORK,
        SKILLS,
        "/etc/lampOS",
        "/dev/shm",
        "/run/systemd/resolve",
    ] {
        fs::create_dir_all(at(dir)).unwrap();
    }
    made(&at);
    fs::write(&resolv, "nameserver 127.0.0.53\n").unwrap();
    let uid_map = folder.path().join("uid_map");
    fs::write(&uid_map, "         0        996          1\n").unwrap();
    let stub = "#!/bin/sh\necho \"${0##*/} $*\" >> \"$LOG\"\n\
                case ${0##*/} in findmnt) echo rw,nosuid,nodev,relatime;; readlink) echo \"$RESOLV\";; \
                unshare) echo ran;; esac\n";
    for name in [
        "mount", "findmnt", "mkdir", "rmdir", "rm", "touch", "readlink", "unshare", "ip",
    ] {
        let path = stubs.join(name);
        fs::write(&path, stub).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut text = ns_text(Shown {
        skills: false,
        tasks: true,
        one_path,
    });
    let moved = [
        "/var/tmp",
        "/var/lib",
        "/etc/lampOS",
        "/etc/resolv.conf",
        "/dev/shm",
        "/tmp",
        "/run",
    ];
    for (n, path) in moved.iter().enumerate() {
        text = text.replace(path, &format!("\u{1}{n}\u{1}"));
    }
    for (n, path) in moved.iter().enumerate() {
        text = text.replace(&format!("\u{1}{n}\u{1}"), &at(path));
    }
    let text = text
        .replace("/proc/self/uid_map", &uid_map.to_string_lossy())
        .replace("/usr/bin/unshare", &stubs.join("unshare").to_string_lossy());
    let script = folder.path().join("openclaw-ns");
    fs::write(&script, text).unwrap();
    let out = std::process::Command::new("/bin/sh")
        .arg(&script)
        .arg("gateway")
        .current_dir(at(WORK))
        .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
        .env("LOG", &log)
        .env("RESOLV", &resolv)
        .env("LAMP_NS_UID", "996")
        .env("LAMP_NS_GID", "994")
        .env_remove("LAMP_NS_NET")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let lines = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    (lines, (at("/tmp/.lampos-ns"), at(START_SOCKET_DIR)))
}

/// The namespace lamp-update writes for OpenClaw with the one run path
/// holds, byte for byte, the line lamp-tasks reads as it showing
/// lamp-control's socket (`lamp_tasks::skill_task::AGENT_NS_TASKS_LINE`,
/// the contract between the two): without it, lamp-tasks refuses every
/// skill's run on OpenClaw as "doesn't show lamp-control's socket"
/// (c0a4's #7133 dry run, 15 Nov, when `Q=` went inside that line).
#[test]
fn the_openclaw_namespace_keeps_the_line_lamp_tasks_reads() {
    const AGENT_NS_TASKS_LINE: &str = " L=/run/lampos-control/control.sock B=/run/lampos-tasks/model.sock P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns\n";
    for skills in [false, true] {
        let ns = ns_text(shown(skills, true));
        assert_eq!(ns.matches(AGENT_NS_TASKS_LINE).count(), 1, "{ns}");
        assert!(ns.contains("\nQ=/run/lampos-skill-start\n"), "{ns}");
    }
}

/// #7135 (V2's HOLD, walk4 on a lamp): a one-shot whose gateway unit is an
/// earlier build's (a binary ahead of its units) waits for the round that
/// writes it: OpenClaw's wrapper, namespace script and gateway stay as
/// they are, nothing is stopped, and the record is no failure. Once the
/// round has written the unit, the same ask installs as before.
#[test]
fn an_earlier_builds_gateway_unit_waits_and_never_takes_openclaw_out() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    round(&lamp, &mut systemd, true, 10).unwrap();
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 20).unwrap().install, "installed");
    // An earlier build's unit and wrapper, as a staged binary finds them.
    fs::write(
        lamp.unit(),
        "[Unit]\nDescription=an earlier build's gateway\n",
    )
    .unwrap();
    fs::write(
        lamp.at(WRAPPER),
        "#!/bin/sh\n# an earlier build's wrapper\n",
    )
    .unwrap();
    let ns_before = fs::read(lamp.at(NS)).unwrap();
    lamp.ask();
    let runs = Runs::new(&lamp);
    let said = install(&lamp, &runs, 30).unwrap();
    assert_ne!(said.install, "failed", "{:#?}", runs.lines());
    assert_eq!(
        fs::read_to_string(lamp.at(WRAPPER)).unwrap(),
        "#!/bin/sh\n# an earlier build's wrapper\n",
        "left as it is"
    );
    assert_eq!(fs::read(lamp.at(NS)).unwrap(), ns_before, "left as it is");
    assert!(
        !runs
            .lines()
            .iter()
            .any(|line| line.contains("systemctl stop")),
        "{:#?}",
        runs.lines()
    );
    // The round writes this build's unit and asks again: installed.
    round(&lamp, &mut systemd, true, 40);
    assert_eq!(
        fs::read_to_string(lamp.unit()).unwrap(),
        unit_text_for(sk(true))
    );
    let runs = Runs::new(&lamp);
    if lamp.asked().is_none() {
        lamp.ask();
    }
    assert_eq!(
        install(&lamp, &runs, 50).unwrap().install,
        "installed",
        "{:#?}",
        runs.lines()
    );
    assert_eq!(
        fs::read_to_string(lamp.at(WRAPPER)).unwrap(),
        wrapper_text_for(sk(true))
    );
}

/// #7135 (c0a4's first run denied): a gateway just started answers the
/// allow-list add "Config unavailable" until its config is read. The add
/// is tried again until it is taken, so the install records the entry and
/// a skill's first run is not denied; any other refusal is not retried
/// in that loop, only run once more after the gateway settled (#8444).
#[test]
fn the_allowlist_add_is_tried_again_while_the_gateways_config_is_not_ready() {
    let mut adds = 0;
    let mut run = |argv: &[String]| -> Result<String, String> {
        if argv.iter().any(|arg| arg == "add") {
            adds += 1;
            if adds < 3 {
                return Err(format!("{CONFIG_NOT_READY}: cannot validate --agent."));
            }
        }
        Ok(String::new())
    };
    let shown = Shown {
        one_path: true,
        ..Shown::default()
    };
    assert_eq!(
        exec_allowlist(Path::new("/nonexistent"), &mut run, shown, &mut quiet()),
        Some(true)
    );
    assert_eq!(adds, 3);

    let mut adds = 0;
    let mut run = |argv: &[String]| -> Result<String, String> {
        if argv.iter().any(|arg| arg == "add") {
            adds += 1;
            return Err("unknown agent".into());
        }
        Ok(String::new())
    };
    assert_eq!(
        exec_allowlist(Path::new("/nonexistent"), &mut run, shown, &mut quiet()),
        Some(false)
    );
    assert_eq!(
        adds, 2,
        "another refusal: one retry after the settle, #8444"
    );
}

/// #7135 (c0a4, 21:22:21, an update's one-shot): the add joined a gateway
/// just restarted whose namespace did not show Node yet, then answered
/// "Config unavailable". Both are tried again in the same one-shot, so the
/// entry lands and a skill's first run after an update is not denied.
#[test]
fn the_allowlist_add_is_tried_again_while_the_gateways_node_is_not_there_yet() {
    let mut adds = 0;
    let mut run = |argv: &[String]| -> Result<String, String> {
        if argv.iter().any(|arg| arg == "add") {
            adds += 1;
            match adds {
                1 | 2 => return Err(format!("/usr/bin/env: {NODE_NOT_READY}")),
                3 => return Err(format!("{CONFIG_NOT_READY}: cannot validate --agent.")),
                _ => {}
            }
        }
        Ok(String::new())
    };
    let shown = Shown {
        one_path: true,
        ..Shown::default()
    };
    assert_eq!(
        exec_allowlist(Path::new("/nonexistent"), &mut run, shown, &mut quiet()),
        Some(true)
    );
    assert_eq!(adds, 4);
}

/// The install run with its gateway started long ago (400 s after a boot
/// 1000 s ago) and `restarts` of its allow-list adds failing because the
/// gateway is restarted under them: the record, what the install said,
/// and its commands.
fn install_restarted_at_add(restarts: u32) -> (OpenclawInstall, Vec<String>, Vec<String>) {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    round_shown(&lamp, &mut Fake::default(), one_path());
    fs::write(lamp.at(crate::reset_hold::UPTIME), "1000.00 3000.00\n").unwrap();
    let runs = Runs::new(&lamp);
    *runs.began.borrow_mut() = vec![format!("{}\n", 400 * 1_000_000)];
    *runs.restart_at_add.borrow_mut() = restarts;
    let mut said = Vec::new();
    let mut run = runs.runner();
    let record = install_with(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 20,
        &mut |line: &str| said.push(line.to_owned()),
        &kit(),
    )
    .unwrap()
    .unwrap();
    drop(run);
    (record, said, runs.lines())
}

/// #8444 (kit-8324i on lamp-52e6, 24 Nov 03:49): lamp-control restarted
/// while the one-shot ran agent main's allow-list add, `PartOf=` restarted
/// the gateway under it, the add failed and skill-start stayed unlisted
/// until a round. The one-shot waits as `settled` does until the gateway
/// is active, listens and is past its first start (150 s from the start
/// at 1000 s: 76 looks, 74 of them `ss` once it is active), then runs the
/// add once more: the entry is there. A retry that fails too is said in
/// its own line; the add is never run a third time.
#[test]
fn an_allowlist_add_failed_by_a_gateway_restart_is_run_again_once_it_settled() {
    let (record, said, lines) = install_restarted_at_add(1);
    let adds: Vec<usize> = (0..lines.len())
        .filter(|&at| lines[at].contains(" approvals allowlist add "))
        .collect();
    assert_eq!(adds.len(), 2, "{lines:#?}");
    let looks = lines[adds[0]..adds[1]]
        .iter()
        .filter(|line| line.starts_with("ss -Hltn"))
        .count();
    assert_eq!(looks, 74, "{lines:#?}");
    assert_eq!(record.install, "installed");
    assert!(record.exec_allowlist, "{said:#?}");
    assert!(!said.iter().any(|line| line.contains("#8444")), "{said:#?}");

    let (record, said, lines) = install_restarted_at_add(2);
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains(" approvals allowlist add "))
            .count(),
        2,
        "{lines:#?}"
    );
    assert!(!record.exec_allowlist);
    assert!(
        said.iter().any(|line| line.starts_with(&format!(
            "OpenClaw's exec allow-list still does not let agent {RUN_AGENT} run {SKILL_START} after the gateway settled and the add ran again: gateway closed (1012)"
        )) && line.ends_with("(#8444)")),
        "{said:#?}"
    );
}

/// #7135 (V2, 52e6 stuck): the package pinned already, the namespace
/// script gone (a failed install took it out), the one-shot writes it
/// again before `--version` runs in it, and installs; never `installer`
/// on every try.
#[test]
fn a_pinned_package_with_its_namespace_script_gone_installs_again() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    round(&lamp, &mut systemd, true, 10).unwrap();
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 20).unwrap().install, "installed");
    fs::remove_file(lamp.at(NS)).unwrap();
    fs::remove_file(lamp.at(WRAPPER)).unwrap();
    lamp.ask();
    let runs = Runs::new(&lamp);
    let said = install(&lamp, &runs, 30).unwrap();
    assert_eq!(said.install, "installed", "{:#?}", runs.lines());
    // The ask as written here: every switch off.
    assert_eq!(
        fs::read_to_string(lamp.at(NS)).unwrap(),
        ns_text(Shown::default())
    );
    assert!(
        !runs.lines().iter().any(|line| line.contains(NPM_CI_ENV)),
        "pinned: no npm ci again"
    );
}

/// Run the wait emitted into the real namespace script, with a real Unix
/// socket and a fake clock, without mounts or root. Not there after 60
/// waits (or a wait interrupted), it logs one line and goes on: never fails.
#[test]
fn gateway_control_socket_wait_is_bounded_and_never_fails() {
    use std::os::unix::net::UnixListener;
    use std::process::Command;
    let ns = ns_text(one_path());
    let start = ns.find("# Wait before the gateway captures").unwrap();
    let end = ns[start..]
        .find("# The bind below shows the socket")
        .unwrap()
        + start;
    let wait = &ns[start..end];
    for (mode, expected, waits) in [
        ("ready", None, 0),
        ("late", None, 1),
        (
            "missing",
            Some("not there after 60 s; the gateway starts without it"),
            60,
        ),
        (
            "file",
            Some("not there after 60 s; the gateway starts without it"),
            60,
        ),
        (
            "interrupted",
            Some("wait interrupted; the gateway starts without it"),
            1,
        ),
        ("install", None, 0),
    ] {
        // Short paths also fit macOS's Unix socket path limit.
        let folder = tempfile::tempdir_in("/tmp").unwrap();
        let socket = folder.path().join("control.sock");
        let ready = folder.path().join("ready.sock");
        let _listener = UnixListener::bind(if mode == "ready" { &socket } else { &ready }).unwrap();
        if mode == "file" {
            fs::write(&socket, "not a socket").unwrap();
        }
        let sleep = folder.path().join("sleep");
        fs::write(&sleep, "#!/bin/sh\necho wait >> \"$WAIT_LOG\"\ncase $MODE in late) ln -s \"$READY_SOCKET\" \"$L\";; interrupted) exit 1;; esac\n").unwrap();
        fs::set_permissions(&sleep, fs::Permissions::from_mode(0o755)).unwrap();
        let log = folder.path().join("waits");
        let output = Command::new("/bin/sh")
            .args([
                "-c",
                &format!("fail() {{ echo \"$*\" >&2; exit 1; }}\n{wait}\necho ready"),
            ])
            .env("PATH", format!("{}:/usr/bin:/bin", folder.path().display()))
            .env("L", &socket)
            .env("READY_SOCKET", &ready)
            .env("WAIT_LOG", &log)
            .env("MODE", mode)
            .env_remove("LAMP_OPENCLAW_GATEWAY")
            .arg("namespace-test")
            .args(if mode == "install" {
                vec!["--version"]
            } else {
                vec!["gateway", "run"]
            })
            .output()
            .unwrap();
        assert!(output.status.success(), "{mode}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("ready"),
            "{mode}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        match expected {
            Some(line) => assert_eq!(
                stderr.trim_end(),
                format!("lampos ns: lamp-control's socket {line}"),
                "{mode}"
            ),
            None => assert_eq!(stderr, "", "{mode}"),
        }
        assert_eq!(
            fs::read_to_string(log).unwrap_or_default().lines().count(),
            waits,
            "{mode}"
        );
    }
}

#[test]
fn gateway_waits_for_control_only_with_the_one_run_path() {
    for skills in [false, true] {
        for tasks in [false, true] {
            for one_path in [false, true] {
                let shown = Shown {
                    skills,
                    tasks,
                    one_path,
                };
                let ns = ns_text(shown);
                let unit = unit_text_for(shown);
                assert_eq!(ns.contains(WAIT_FOR_CONTROL), tasks && one_path);
                assert_eq!(
                    unit.contains("Wants=lamp-control.service\n"),
                    tasks && one_path
                );
                if tasks && one_path {
                    assert!(unit.contains(
                        "After=network-online.target lamp-cloud.service lamp-control.service\n"
                    ));
                    assert!(
                        ns.find(WAIT_FOR_CONTROL).unwrap()
                            < ns.find("mount --bind \"$L\"").unwrap()
                    );
                    assert!(!ns.contains("disappeared before bind"));
                    assert!(!WAIT_FOR_CONTROL.contains("fail"));
                }
            }
        }
    }
}

/// #8320: the gateway's namespace binds lamp-control's socket file once,
/// at its start, and each lamp-control start makes a new one, so with that
/// socket shown the gateway is `PartOf=` lamp-control (restarted and
/// stopped with it), in its `[Unit]` section; without it, not.
#[test]
fn the_gateway_restarts_with_lamp_control_when_it_shows_its_socket() {
    for skills in [false, true] {
        for tasks in [false, true] {
            for one_path in [false, true] {
                let unit = unit_text_for(Shown {
                    skills,
                    tasks,
                    one_path,
                });
                let part_of = unit
                    .lines()
                    .position(|line| line == "PartOf=lamp-control.service");
                assert_eq!(part_of.is_some(), tasks && one_path, "{unit}");
                if let Some(at) = part_of {
                    let unit_section = unit.lines().position(|line| line == "[Unit]").unwrap();
                    let service = unit.lines().position(|line| line == "[Service]").unwrap();
                    assert!(unit_section < at && at < service, "{unit}");
                    assert_eq!(unit.matches("PartOf=").count(), 1, "{unit}");
                }
            }
        }
    }
}

/// #8320 (the Reviewer-lead's P3 2): each lamp-control start restarts the
/// gateway (`PartOf=`), so lamp-control gives the gateway its start count
/// back first, as root, a missing gateway no error; else a lamp-control
/// crash loop leaves the gateway at `start-limit-hit` once it is up again.
#[test]
fn lamp_control_gives_the_gateway_its_start_count_back() {
    let control = include_str!("../../../../deploy/product/lamp-control.service");
    let line =
        format!("ExecStartPost=-+/bin/sh -c '/usr/bin/systemctl reset-failed {UNIT} 2>/dev/null'");
    assert_eq!(
        control.lines().filter(|l| *l == line).count(),
        1,
        "{control}"
    );
    let service = control.find("[Service]").unwrap();
    let install = control.find("[Install]").unwrap();
    let at = control.find(&line).unwrap();
    assert!(service < at && at < install);
}

/// A round of [`after_round`] with the switch on and the lines it said.
fn round_said(lamp: &Lamp, systemd: &mut Fake) -> Vec<String> {
    let runs = Runs::new(lamp);
    let mut said = Vec::new();
    after_round(
        lamp.root(),
        &lamp.records(),
        sk(false),
        systemd,
        &mut runs.runner(),
        1,
        Ok(crate::hermes_schedule::Window::Reached),
        &mut |line: &str| said.push(line.to_owned()),
    );
    said
}

/// The lines naming the gateway's unit as not loaded as written.
fn not_loaded(said: &[String]) -> Vec<&String> {
    said.iter()
        .filter(|line| line.contains("not loaded as written") || line.contains("drop-in"))
        .collect()
}

/// #7879: a reload that failed after the unit was written is tried again
/// by the next round, which finds the same bytes but a reload still owed.
#[test]
fn a_failed_reload_after_the_write_is_tried_again_next_round() {
    let lamp = Lamp::new();
    let owed =
        "DropInPaths=\nNeedDaemonReload=yes\nCPUQuotaPerSecUSec=infinity\nActiveState=inactive\n";
    let mut systemd = Fake {
        loaded: Some(owed),
        reloads_fail: 2,
        ..Fake::default()
    };
    let said = round_said(&lamp, &mut systemd);
    assert!(
        said.iter()
            .any(|line| line.contains("systemd not reloaded"))
    );
    assert_eq!(fs::read_to_string(lamp.unit()).unwrap(), UNIT_TEXT);

    let mut systemd = Fake {
        loaded: Some(owed),
        ..Fake::default()
    };
    let said = round_said(&lamp, &mut systemd);
    assert_eq!(
        systemd.asked,
        ["reload"],
        "reloaded again; not running, so not restarted"
    );
    let lines = not_loaded(&said);
    assert_eq!(lines.len(), 1, "{said:?}");
    assert!(lines[0].contains("NeedDaemonReload"), "{}", lines[0]);
}

/// #7879: a drop-in at the gateway's unit is not lamp-update's: said, left
/// as it is, and neither a reload nor a restart, which would not take it out.
#[test]
fn a_drop_in_at_the_gateway_is_said_and_left_as_it_is() {
    let lamp = Lamp::new();
    fs::write(lamp.unit(), UNIT_TEXT).unwrap();
    let folder = lamp.at("/etc/systemd/system/lamp-openclaw-gateway.service.d");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("cap.conf"), "[Service]\nCPUQuota=\n").unwrap();
    let mut systemd = Fake {
        loaded: Some(
            "DropInPaths=/etc/systemd/system/lamp-openclaw-gateway.service.d/cap.conf\n\
             NeedDaemonReload=no\nCPUQuotaPerSecUSec=infinity\nActiveState=active\n",
        ),
        ..Fake::default()
    };
    let said = round_said(&lamp, &mut systemd);
    assert!(systemd.asked.is_empty(), "{:?}", systemd.asked);
    let lines = not_loaded(&said);
    assert_eq!(lines.len(), 1, "{said:?}");
    assert!(
        lines[0].contains("drop-in") && lines[0].contains("cap.conf"),
        "{}",
        lines[0]
    );
    assert!(folder.join("cap.conf").exists(), "never deleted");
}

/// #7879: the CPU cap read back as `infinity` with the gateway running:
/// systemd reloaded and the gateway try-restarted, so the cap applies.
#[test]
fn a_cap_read_back_as_infinity_reloads_and_restarts_a_running_gateway() {
    let lamp = Lamp::new();
    fs::write(lamp.unit(), UNIT_TEXT).unwrap();
    let mut systemd = Fake {
        loaded: Some(
            "DropInPaths=\nNeedDaemonReload=no\nCPUQuotaPerSecUSec=infinity\nActiveState=active\n",
        ),
        ..Fake::default()
    };
    let said = round_said(&lamp, &mut systemd);
    assert_eq!(systemd.asked, ["reload", "try-restart"]);
    let lines = not_loaded(&said);
    assert_eq!(lines.len(), 1, "{said:?}");
    assert!(
        lines[0].contains("CPU cap") && lines[0].contains("infinity"),
        "{}",
        lines[0]
    );
}

/// #7879: the unit loaded as written: checked, and nothing else done.
#[test]
fn a_gateway_unit_loaded_as_written_is_left_alone() {
    let lamp = Lamp::new();
    fs::write(lamp.unit(), UNIT_TEXT).unwrap();
    let mut systemd = Fake {
        loaded: Some(
            "DropInPaths=\nNeedDaemonReload=no\nCPUQuotaPerSecUSec=1s\nActiveState=active\n",
        ),
        ..Fake::default()
    };
    let said = round_said(&lamp, &mut systemd);
    assert_eq!(systemd.shown, 1, "checked once");
    assert!(systemd.asked.is_empty(), "{:?}", systemd.asked);
    assert!(not_loaded(&said).is_empty(), "{said:?}");
}

/// The `ss` looks the install made, and whether all came before its one
/// gateway restart.
fn settle_looks(runs: &Runs) -> usize {
    let lines = runs.lines();
    let restart = runs.position(&format!("systemctl restart {UNIT}"));
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("systemctl restart"))
            .count(),
        1,
        "{lines:#?}"
    );
    let looks: Vec<usize> = (0..lines.len())
        .filter(|&at| lines[at].starts_with("ss -Hltn"))
        .collect();
    assert!(looks.iter().all(|&at| at < restart), "{lines:#?}");
    looks.len()
}

/// The install run on a lamp the round asked it of, 1000 s after the boot,
/// its gateway's process started at `began` (seconds since the boot, one
/// per look, the last kept; none: not said) and not listening for
/// `quiet_looks` looks.
fn install_settling(began: &[u64], quiet_looks: u32) -> usize {
    let lamp = Lamp::new();
    lamp.harness_there();
    round(&lamp, &mut Fake::default(), true, 10);
    fs::write(lamp.at(crate::reset_hold::UPTIME), "1000.00 3000.00\n").unwrap();
    let runs = Runs::new(&lamp);
    *runs.not_listening.borrow_mut() = quiet_looks;
    *runs.began.borrow_mut() = began
        .iter()
        .map(|s| format!("{}\n", s * 1_000_000))
        .collect();
    let mut run = runs.runner();
    let said = install_with(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 20,
        &mut quiet(),
        &kit(),
    )
    .unwrap()
    .unwrap();
    drop(run);
    assert_eq!(said.install, "installed");
    settle_looks(&runs)
}

/// #8324 (the Reviewer-lead's HOLD on 2f22d4200b): a stop at the moment
/// :18789 first listens is the 79.9 s drain (kit-8306b on lamp-52e6), so
/// the install's restart of a gateway started 10 s ago, which listens on
/// the 4th look, waits past the listen until 150 s from its start (and 90 s
/// past that listen): 71 looks 2 s apart, all before its one restart.
#[test]
fn the_install_never_restarts_a_gateway_at_its_first_listen() {
    assert_eq!(install_settling(&[990], 3), 71);
    // Listening already, 10 s into its start (the stop right at first
    // listen): still waited out to 150 s from the start.
    assert_eq!(install_settling(&[990], 0), 71);
}

/// #8324: a gateway started long ago and listening is restarted at once;
/// one whose start systemd can't say, seen to begin listening on the 4th
/// look, is restarted 90 s past that listen; one never listening is
/// restarted anyway after 300 s.
#[test]
fn the_install_restarts_a_settled_gateway_at_once_and_waits_past_a_new_listen() {
    assert_eq!(install_settling(&[400], 0), 1);
    assert_eq!(install_settling(&[], 0), 1);
    assert_eq!(install_settling(&[], 3), 3 + 45 + 1);
    assert_eq!(install_settling(&[], u32::MAX), 151);
}

/// #8324 (the Reviewer-lead's P3 1 on 5ae65157cf): a gateway started again
/// during the wait (`Restart=always` after a crash) is waited for from its
/// new start, read again at each look: started 10 s before, then again
/// 90 s into the wait (at the 51st look), it is restarted 150 s after that
/// second start, never at the first start's 150 s.
#[test]
fn a_gateway_started_again_during_the_wait_is_waited_for_from_its_new_start() {
    let mut began = vec![990; 50];
    began.push(1090);
    assert_eq!(install_settling(&began, 0), 121);
}

/// #8320 (the first builder's and the Reviewer-lead's HOLD): the round
/// that ships `PartOf=` restarts lamp-control before it writes the
/// gateway's unit, so a running gateway showing lamp-control's socket is
/// try-restarted once when its unit is written anew; the next round, the
/// same bytes, restarts nothing.
#[test]
fn a_gateway_unit_written_anew_with_the_control_socket_restarts_it_once() {
    let lamp = Lamp::new();
    let shown = one_path();
    assert!(shown.tasks && shown.one_path);
    let before = unit_text_for(shown).replacen("PartOf=lamp-control.service\n", "", 1);
    fs::write(lamp.unit(), before).unwrap();
    let running = "DropInPaths=\nNeedDaemonReload=no\nCPUQuotaPerSecUSec=1s\nActiveState=active\n";
    let round = |systemd: &mut Fake| {
        let runs = Runs::new(&lamp);
        let mut said = Vec::new();
        after_round(
            lamp.root(),
            &lamp.records(),
            shown,
            systemd,
            &mut runs.runner(),
            1,
            Ok(crate::hermes_schedule::Window::Reached),
            &mut |line: &str| said.push(line.to_owned()),
        );
        said
    };
    let mut systemd = Fake {
        loaded: Some(running),
        ..Fake::default()
    };
    let said = round(&mut systemd);
    assert_eq!(
        fs::read_to_string(lamp.unit()).unwrap(),
        unit_text_for(shown)
    );
    let restarts = |systemd: &Fake| {
        systemd
            .asked
            .iter()
            .filter(|a| **a == "try-restart")
            .count()
    };
    assert_eq!(restarts(&systemd), 1, "{:?}", systemd.asked);
    assert!(said.iter().any(|line| line.contains("#8320")), "{said:?}");

    let mut systemd = Fake {
        loaded: Some(running),
        ..Fake::default()
    };
    round(&mut systemd);
    assert_eq!(restarts(&systemd), 0, "{:?}", systemd.asked);

    // A unit written anew over one that had `PartOf=` already: lamp-control's
    // restart in the round's switch restarted the gateway, so no second
    // stop lands in its first start (#8324, gate G).
    fs::write(
        lamp.unit(),
        format!("{}# an earlier text\n", unit_text_for(shown)),
    )
    .unwrap();
    let mut systemd = Fake {
        loaded: Some(running),
        ..Fake::default()
    };
    let said = round(&mut systemd);
    assert_eq!(
        fs::read_to_string(lamp.unit()).unwrap(),
        unit_text_for(shown)
    );
    assert_eq!(restarts(&systemd), 0, "{:?}", systemd.asked);
    assert!(!said.iter().any(|line| line.contains("#8320")), "{said:?}");
}

/// #8105: lamp-tasks' answer read, and the rule: the switch off is always
/// wanted; on, OpenClaw the default is wanted, another default is not, and
/// no answer or no default leaves the gateway as it is. An older
/// lamp-tasks' `openclaw_in_use` is not read: the default alone decides,
/// so no hold keeps OpenClaw's gateway up beside Hermes's.
#[test]
fn the_gateway_is_wanted_while_openclaw_is_the_lamps_agent() {
    let said = |default: Option<&str>| Said {
        default: default.map(str::to_owned),
    };
    assert_eq!(
        default_of(r#"{"ok":true,"default":"openclaw","chosen_in_app":true}"#),
        Some(said(Some("openclaw")))
    );
    assert_eq!(
        default_of(r#"{"ok":true,"default":"hermes","openclaw_in_use":true}"#),
        Some(said(Some("hermes"))),
        "an older lamp-tasks' in-use field is not read"
    );
    assert_eq!(
        default_of(r#"{"ok":true,"default":null,"openclaw_in_use":true}"#),
        Some(said(None))
    );
    assert_eq!(default_of(r#"{"ok":true,"default":" "}"#), Some(said(None)));
    assert_eq!(default_of(r#"{"ok":false,"error":"busy"}"#), None);
    assert_eq!(default_of("not json"), None);

    let rule = |default| gateway_wanted(true, Some(said(default)));
    assert_eq!(
        gateway_wanted(false, Some(said(Some("hermes")))),
        Gateway::Wanted
    );
    assert_eq!(gateway_wanted(false, None), Gateway::Wanted);
    assert_eq!(rule(Some("openclaw")), Gateway::Wanted);
    assert_eq!(rule(Some("hermes")), Gateway::NotWanted);
    assert_eq!(rule(Some("claude")), Gateway::NotWanted);
    assert_eq!(rule(None), Gateway::Unknown);
    assert_eq!(gateway_wanted(true, None), Gateway::Unknown);
}

/// #8105: lamp-tasks asked on its app socket as lamp-cloud asks it, one
/// line each way; nothing listening is no answer.
#[test]
fn lamp_tasks_is_asked_for_the_default_on_its_app_socket() {
    use std::io::{BufRead, BufReader, Write};
    let folder = tempfile::tempdir().unwrap();
    let socket = folder.path().join("app.sock");
    assert_eq!(said_default(&socket), None, "nobody listening");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let served = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        (&stream)
            .write_all(
                b"{\"ok\":true,\"default\":\"hermes\",\"chosen_in_app\":false,\"openclaw_in_use\":false}\n",
            )
            .unwrap();
        line
    });
    assert_eq!(
        said_default(&socket),
        Some(Said {
            default: Some("hermes".to_owned()),
        })
    );
    assert_eq!(
        served.join().unwrap(),
        "{\"op\":\"app_engine_default_state\"}\n"
    );
}

/// #8105: a round with OpenClaw there that doesn't want the gateway
/// disables and stops it and never starts it; unknown leaves it as it is;
/// wanted again enables and starts it.
#[test]
fn a_round_on_a_lamp_whose_agent_is_not_openclaw_stops_the_gateway() {
    let lamp = Lamp::new();
    lamp.harness_there();
    round(&lamp, &mut Fake::default(), true, 1);
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 2).unwrap().install, "installed");
    let mut systemd = Fake::default();
    round(&lamp, &mut systemd, true, 3);
    assert!(
        fs::symlink_metadata(lamp.at(WANTS)).is_ok(),
        "enabled as before"
    );

    let at = |systemd: &mut Fake, gateway: Gateway, runs: &Runs, at: u64| {
        after_round_with(
            lamp.root(),
            &lamp.records(),
            sk(true),
            gateway,
            true,
            systemd,
            &mut runs.runner(),
            at,
            Ok(crate::hermes_schedule::Window::Reached),
            &mut quiet(),
        )
    };
    let mut systemd = Fake {
        gateway: Some("active"),
        ..Fake::default()
    };
    let runs = Runs::new(&lamp);
    at(&mut systemd, Gateway::NotWanted, &runs, 4);
    assert!(
        fs::symlink_metadata(lamp.at(WANTS)).is_err(),
        "not started at boot"
    );
    assert!(
        systemd.asked.contains(&"stop-gateway"),
        "{:?}",
        systemd.asked
    );
    assert!(
        !systemd.asked.contains(&"start-gateway"),
        "{:?}",
        systemd.asked
    );
    assert!(
        !runs.lines().iter().any(|line| line.contains("approvals")),
        "nothing goes through a gateway that is stopped: {:?}",
        runs.lines()
    );

    // Unknown: left as it is, nothing asked of the gateway.
    let mut systemd = Fake::default();
    at(&mut systemd, Gateway::Unknown, &Runs::new(&lamp), 5);
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_err());
    assert!(
        !systemd.asked.iter().any(|asked| asked.contains("gateway")),
        "{:?}",
        systemd.asked
    );

    // Wanted again (OpenClaw chosen): enabled and started.
    let mut systemd = Fake::default();
    at(&mut systemd, Gateway::Wanted, &Runs::new(&lamp), 6);
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_ok());
    assert!(
        systemd.asked.contains(&"start-gateway"),
        "{:?}",
        systemd.asked
    );
}

/// #8105: between rounds, each [`KEEP_EVERY`]: a gateway already as wanted
/// is left with no word and no systemctl but its state; one that isn't is
/// started or stopped, and said once.
#[test]
fn between_rounds_the_gateway_follows_the_lamps_agent_and_is_quiet_when_settled() {
    let lamp = Lamp::new();
    lamp.harness_there();
    round(&lamp, &mut Fake::default(), true, 1);
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 2).unwrap().install, "installed");
    round(&lamp, &mut Fake::default(), true, 3);
    let keep = |gateway: Gateway, state: &'static str| {
        let mut systemd = Fake {
            gateway: Some(state),
            active: Some("inactive"),
            ..Fake::default()
        };
        let mut said = Vec::new();
        keep_between_rounds(
            lamp.root(),
            &mut systemd,
            gateway,
            false,
            &mut Runs::new(&lamp).runner(),
            &mut |line: &str| said.push(line.to_owned()),
        );
        (systemd.asked, said)
    };
    // OpenClaw the agent, the gateway up and enabled: nothing done.
    let (asked, said) = keep(Gateway::Wanted, "active");
    assert_eq!(asked, ["state", "gateway-state"]);
    assert!(said.is_empty(), "{said:?}");
    // Hermes chosen: disabled and stopped, said.
    let (asked, said) = keep(Gateway::NotWanted, "active");
    assert!(asked.contains(&"stop-gateway"), "{asked:?}");
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_err());
    assert!(said.iter().any(|line| line.contains("stopped")), "{said:?}");
    // Settled down: nothing more.
    let (asked, said) = keep(Gateway::NotWanted, "inactive");
    assert_eq!(asked, ["state", "gateway-state"]);
    assert!(said.is_empty(), "{said:?}");
    // lamp-tasks not reached: left down.
    let (asked, _) = keep(Gateway::Unknown, "inactive");
    assert_eq!(asked, ["state", "gateway-state"]);
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_err());
    // OpenClaw chosen in the app: enabled and started, said.
    let (asked, said) = keep(Gateway::Wanted, "inactive");
    assert!(asked.contains(&"start-gateway"), "{asked:?}");
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_ok());
    assert!(said.iter().any(|line| line.contains("started")), "{said:?}");
    // Harness's one-shot at work: a wanted gateway is left to it...
    let mut systemd = Fake {
        gateway: Some("inactive"),
        active: Some("activating"),
        ..Fake::default()
    };
    fs::remove_file(lamp.at(WANTS)).unwrap();
    keep_between_rounds(
        lamp.root(),
        &mut systemd,
        Gateway::Wanted,
        false,
        &mut Runs::new(&lamp).runner(),
        &mut quiet(),
    );
    assert_eq!(systemd.asked, ["state"]);
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_err());
    // ...but one not wanted is still stopped and disabled: another engine's
    // gateway waits for it (one agent gateway at a time).
    fs::write(lamp.at(WANTS), "").unwrap();
    let mut systemd = Fake {
        gateway: Some("active"),
        active: Some("activating"),
        ..Fake::default()
    };
    keep_between_rounds(
        lamp.root(),
        &mut systemd,
        Gateway::NotWanted,
        false,
        &mut Runs::new(&lamp).runner(),
        &mut quiet(),
    );
    assert!(
        systemd.asked.contains(&"stop-gateway"),
        "{:?}",
        systemd.asked
    );
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_err());
}

/// Twelve's P2 on #8116: a gateway started between rounds because OpenClaw
/// became the lamp's agent gets agent `main`'s entry added at once with the
/// one run path on, through the running gateway; settled, nothing is run.
#[test]
fn a_gateway_started_for_openclaw_gets_agent_mains_entry_at_once() {
    let lamp = Lamp::new();
    lamp.harness_there();
    tasks_there(&lamp, TASKS_BYTES);
    let mut systemd = Fake::default();
    round_shown(&lamp, &mut systemd, one_path());
    install(&lamp, &Runs::new(&lamp), 20).unwrap();
    let mut idle = Fake {
        gateway: Some("active"),
        active: Some("inactive"),
        ..Fake::default()
    };
    keep_between_rounds(
        lamp.root(),
        &mut idle,
        Gateway::NotWanted,
        true,
        &mut Runs::new(&lamp).runner(),
        &mut quiet(),
    );
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_err());

    let runs = Runs::new(&lamp);
    let mut down = Fake {
        gateway: Some("inactive"),
        active: Some("inactive"),
        ..Fake::default()
    };
    keep_between_rounds(
        lamp.root(),
        &mut down,
        Gateway::Wanted,
        true,
        &mut runs.runner(),
        &mut quiet(),
    );
    assert!(down.asked.contains(&"start-gateway"), "{:?}", down.asked);
    assert_eq!(
        approvals(&runs),
        [approvals_line("add", "main", SKILL_START)]
    );

    let runs = Runs::new(&lamp);
    let mut up = Fake {
        gateway: Some("active"),
        active: Some("inactive"),
        ..Fake::default()
    };
    keep_between_rounds(
        lamp.root(),
        &mut up,
        Gateway::Wanted,
        true,
        &mut runs.runner(),
        &mut quiet(),
    );
    assert_eq!(
        approvals(&runs),
        Vec::<String>::new(),
        "settled: nothing run"
    );
}

/// #7868 (the first builder, 02:02, point 3), as #7268's `write_waiting`:
/// the one-shot's check of the `gateway` block joins the running gateway,
/// and one just (re)started may not be there to join yet (the wrapper's
/// "is not running in its own namespaces", or "'node': No such file or
/// directory"). Only that refusal is tried again, a bounded number of
/// times; then the block lands and the gateway is restarted on it. Past
/// the last try it is not written and the next round asks again.
#[test]
fn a_gateway_block_the_gateway_was_not_there_to_check_is_tried_again() {
    let no_gateway = "openclaw: its gateway (lamp-openclaw-gateway.service) is not running in its own namespaces; nothing runs without it";
    for (refusals, lands) in [(2, true), (100, false)] {
        let lamp = installed_openclaw();
        let mut config = config_of(&lamp);
        config.as_object_mut().unwrap().remove("gateway");
        let before = config.to_string();
        fs::write(lamp.at(CONFIG), &before).unwrap();
        a_round_asks_the_one_shot(&lamp);
        let runs = Runs {
            main_pid: "4242",
            ..Runs::new(&lamp)
        };
        let mut inner = runs.runner();
        let mut checks = 0;
        let mut said = Vec::new();
        install_with(
            lamp.root(),
            &lamp.records(),
            &mut |argv: &[String]| {
                let answer = inner(argv);
                if argv.join(" ").contains("config validate --json") {
                    checks += 1;
                    if checks <= refusals {
                        return Err(no_gateway.to_owned());
                    }
                }
                answer
            },
            &mut || 40,
            &mut |line: &str| said.push(line.to_owned()),
            &Kit::BUILT,
        )
        .unwrap();
        drop(inner);
        let restarted = runs
            .lines()
            .iter()
            .any(|line| line.contains(&format!("try-restart -- {UNIT}")));
        if lands {
            assert_eq!(checks, 3, "{said:#?}");
            assert_eq!(config_of(&lamp)["gateway"], installs_block(&lamp));
            assert!(restarted);
        } else {
            assert!((2..=20).contains(&checks), "bounded: {checks}");
            assert_eq!(fs::read_to_string(lamp.at(CONFIG)).unwrap(), before);
            assert!(!restarted);
            assert!(
                said.iter().any(|line| line.contains("not written again")),
                "{said:#?}"
            );
        }
    }
}

/// The words lamp-tasks' and the one-shot's retry look for
/// (`lamp_checked_replace::NO_GATEWAY`) are the words the installed
/// wrapper says when it finds no gateway to join, so a reworded wrapper
/// can't turn every retry into a final refusal (#7872's P3).
#[test]
fn the_wrappers_no_gateway_words_are_the_ones_the_retry_looks_for() {
    for text in [
        wrapper_text(),
        wrapper_text_for(Shown {
            one_path: true,
            ..Shown::default()
        }),
    ] {
        let line = text
            .lines()
            .find(|line| line.contains("lamp-openclaw-gateway.service) is not running"))
            .expect("the wrapper's no-gateway line");
        assert!(line.contains(lamp_checked_replace::NO_GATEWAY), "{line}");
    }
}

/// The commands of `runs` that start or restart OpenClaw's gateway.
fn gateway_starts(runs: &Runs) -> Vec<String> {
    runs.lines()
        .into_iter()
        .filter(|line| {
            line.starts_with("systemctl ")
                && line.ends_with(UNIT)
                && [" start ", " restart ", " try-restart "]
                    .iter()
                    .any(|verb| line.contains(verb))
        })
        .collect()
}

fn said_is(default: &str) -> Option<Said> {
    Some(Said {
        default: Some(default.to_owned()),
    })
}

/// One agent gateway at a time: the install a round asks with the gateway
/// following the default (`by_default`, handed in the ask) never starts or
/// restarts OpenClaw's gateway while lamp-tasks says another engine is the
/// lamp's agent (Harness's one-shot used to restart it whatever the
/// default); it still does while OpenClaw is, and always with the switch
/// off, as before.
#[test]
fn an_install_starts_the_gateway_only_while_openclaw_is_wanted() {
    for (by_default, default, starts) in [
        (true, "hermes", false),
        (true, "openclaw", true),
        (false, "hermes", true),
    ] {
        let lamp = Lamp::new();
        lamp.harness_there();
        let mut systemd = Fake::default();
        after_round_with(
            lamp.root(),
            &lamp.records(),
            sk(true),
            gateway_wanted(by_default, said_is(default)),
            by_default,
            &mut systemd,
            &mut Runs::new(&lamp).runner(),
            1,
            Ok(crate::hermes_schedule::Window::Reached),
            &mut quiet(),
        );
        assert_eq!(lamp.asked().map(|ask| ask.by_default), Some(by_default));
        let runs = Runs::new(&lamp);
        let said = install_said(
            lamp.root(),
            &lamp.records(),
            &mut runs.runner(),
            &mut || 2,
            &mut quiet(),
            &kit(),
            &mut || said_is(default),
        )
        .unwrap()
        .unwrap();
        assert_eq!(said.install, "installed", "{by_default}/{default}");
        assert_eq!(
            !gateway_starts(&runs).is_empty(),
            starts,
            "{by_default}/{default}: {:#?}",
            runs.lines()
        );
    }
}

/// One agent gateway at a time, an OpenClaw there: a round that doesn't
/// want the gateway asks no one-shot for the gateway block (written and
/// checked through the running gateway, then restarted on it); one that
/// does, does. And a one-shot asked while OpenClaw was wanted, run after
/// the default moved to another engine, writes the block but never
/// restarts the gateway.
#[test]
fn the_gateway_block_restarts_no_gateway_that_is_not_wanted() {
    let block_gone = |lamp: &Lamp| {
        let mut config = config_of(lamp);
        config.as_object_mut().unwrap().remove("gateway");
        fs::write(lamp.at(CONFIG), config.to_string()).unwrap();
    };
    let lamp = installed_openclaw();
    block_gone(&lamp);
    let round = |gateway: Gateway| {
        let mut systemd = Fake {
            gateway: Some("inactive"),
            ..Fake::default()
        };
        after_round_with(
            lamp.root(),
            &lamp.records(),
            sk(true),
            gateway,
            true,
            &mut systemd,
            &mut Runs::new(&lamp).runner(),
            30,
            Ok(crate::hermes_schedule::Window::Reached),
            &mut quiet(),
        );
        systemd.asked
    };
    let asked = round(Gateway::NotWanted);
    assert!(
        lamp.asked().is_none(),
        "no one-shot for a gateway not wanted"
    );
    assert!(!asked.contains(&"start"), "{asked:?}");
    round(Gateway::Wanted);
    assert!(lamp.asked().is_some_and(|ask| ask.by_default));

    // The default moved to Hermes before the one-shot ran.
    let runs = Runs {
        main_pid: "4242",
        ..Runs::new(&lamp)
    };
    let mut said = Vec::new();
    install_said(
        lamp.root(),
        &lamp.records(),
        &mut runs.runner(),
        &mut || 40,
        &mut |line: &str| said.push(line.to_owned()),
        &Kit::BUILT,
        &mut || said_is("hermes"),
    )
    .unwrap();
    assert!(
        said.iter()
            .any(|line| line.contains("gateway block") && line.contains("not restarted")),
        "{said:#?}"
    );
    assert_eq!(gateway_starts(&runs), Vec::<String>::new());
}
