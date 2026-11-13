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
}

impl Systemd for Fake {
    fn states(&mut self, units: &[String]) -> Result<Vec<UnitState>, String> {
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
/// holding it fail; the rest with nothing.
struct Runs {
    root: PathBuf,
    log: Rc<RefCell<Vec<String>>>,
    node: &'static str,
    package: &'static str,
    fail_on: Option<&'static str>,
    /// What the failing command answers.
    error: &'static str,
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
        }
    }
    fn runner(&self) -> impl FnMut(&[String]) -> Result<String, String> + '_ {
        move |argv: &[String]| {
            let line = argv.join(" ");
            self.log.borrow_mut().push(line.clone());
            if self.fail_on.is_some_and(|wanted| line.contains(wanted)) {
                return Err(self.error.to_owned());
            }
            let (uid, gid) = own_ids();
            if line == "id -g lampos" {
                Ok(format!("{gid}\n"))
            } else if argv[0] == "id" {
                Ok(format!("{uid}\n"))
            } else if argv[0] == "df" {
                Ok(
                    "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                    /dev/mmcblk0p2 60000000 1000 20000000 3% /\n"
                        .to_owned(),
                )
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
    after_round(
        lamp.root(),
        &lamp.records(),
        sk(skills),
        systemd,
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
            one_path: false
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
        serde_json::json!({"gateway": {
            "mode": "local",
            "bind": "loopback",
            "port": 18789,
            "auth": {"mode": "token", "token": token},
        }}),
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
        assert!(with_mode.ends_with(&format!("{NPM_CI_MODE}{RUN_LINE}")));
        let checked = std::process::Command::new("/bin/sh")
            .args(["-n", "-c", &with_mode])
            .output()
            .unwrap();
        assert!(checked.status.success(), "{checked:?}");
        let ours = with_mode.replacen(NPM_CI_MODE, "", 1);
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
/// state, started again on failure, below the lamp's services with caps.
#[test]
fn the_gateway_unit_runs_through_the_wrapper_as_lampos_with_caps() {
    let lines: Vec<&str> = UNIT_TEXT.lines().collect();
    for wanted in [
        "User=lampos",
        "Group=lampos",
        "ExecStart=/usr/local/bin/openclaw gateway run",
        "WorkingDirectory=/var/lib/lampos-openclaw/state",
        "Restart=on-failure",
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

/// Each fetch, the unpacking and `npm ci` get [`STEP_WITHIN`]; OpenClaw's
/// other steps the plain bound.
#[test]
fn openclaw_s_long_steps_get_the_long_bound() {
    let downloads = Downloads::make().unwrap();
    let file = downloads.file("node.tar.gz").to_string_lossy().into_owned();
    let unpack = as_lampos(&["tar", "-xzf", file.as_str()]);
    assert!(runs_long_step(&unpack));
    assert_eq!(install::bound_of(&unpack), STEP_WITHIN);
    let npm = in_namespace("1000", "1000", true, &[]);
    assert!(runs_long_step(&npm));
    assert_eq!(install::bound_of(&npm), STEP_WITHIN);
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

/// Turns: Hermes's install under way holds OpenClaw's ask to a later round,
/// and OpenClaw's holds Hermes's; one stuck past its bound holds nothing.
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
    assert_eq!(lamp.asked(), None, "no ask while Hermes's runs");
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

/// (the owner on #6226: every lamp has 6 GB) The gateway is always on with
/// the switch on: once OpenClaw is there the round enables it (the
/// `multi-user.target.wants` link `systemctl enable` makes) and starts it,
/// each round after only starts it (a no-op when it runs); the switch off
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
    assert_eq!(systemd.asked, ["reload", "start-gateway"]);
    let mut systemd = Fake::default();
    round(&lamp, &mut systemd, true, 4);
    assert_eq!(systemd.asked, ["start-gateway"], "enabled once");

    let mut systemd = Fake::default();
    after_round_off(lamp.root(), &lamp.records(), &mut systemd, &mut quiet());
    assert!(fs::symlink_metadata(lamp.at(WANTS)).is_err(), "disabled");
    assert_eq!(systemd.asked[..2], ["reload", "stop-gateway"]);
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
        " L=/run/lampos-control/control.sock B=/run/lampos-tasks/model.sock P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns\n"
    ));
    assert!(!ns_text(sk(true)).contains("L=/run/lampos-control/control.sock"));
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
