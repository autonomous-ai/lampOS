use super::*;
use crate::judge::UnitState;
use std::{cell::RefCell, os::unix::fs::symlink, rc::Rc};

/// Nothing shown past [`ALLOWED`]: both switches off.
const OFF: Shown = Shown {
    skills: false,
    tasks: false,
    one_path: false,
};
/// [`SKILLS_SWITCH`] on alone.
const SKILLS_ON: Shown = Shown {
    skills: true,
    tasks: false,
    one_path: false,
};
/// [`TASKS_SWITCH`] on alone.
const TASKS_ON: Shown = Shown {
    skills: false,
    tasks: true,
    one_path: false,
};

/// What a round asked systemd, in order.
#[derive(Default)]
struct Fake {
    asked: Vec<&'static str>,
    start: Option<Result<(), String>>,
    /// Harness's one-shot's `ActiveState`, for the watch's acts; none: no
    /// state is asked.
    active: Option<&'static str>,
    /// What freeze and thaw answer.
    lever: Option<Result<(), String>>,
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
        self.lever.clone().unwrap_or(Ok(()))
    }
    fn thaw_harness_unit(&mut self) -> Result<(), String> {
        self.asked.push("thaw");
        self.lever.clone().unwrap_or(Ok(()))
    }
    fn stop(&mut self, unit: &str) -> Result<(), String> {
        assert_eq!(unit, harness_install::UNIT);
        self.asked.push("stop");
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
        unreachable!("Harness's unit is started, never restarted")
    }
    fn last_line(&mut self, _: &str) -> Option<String> {
        unreachable!()
    }
    fn harness_unit_properties(&mut self) -> Result<String, String> {
        self.asked.push("show");
        Ok(GOOD.to_owned())
    }
    fn start_harness_unit(&mut self) -> Result<(), String> {
        self.asked.push("start");
        self.start.clone().unwrap_or(Ok(()))
    }
    fn start_writer_unit(&mut self) -> Result<(), String> {
        unreachable!("never the writer's")
    }
}

const GOOD: &str = "Id=lamp-update-harness.service\n\
    LoadState=loaded\n\
    FragmentPath=/etc/systemd/system/lamp-update-harness.service\n\
    DropInPaths=\n\
    UnitFileState=static\n";

const HOUR_MS: u64 = 60 * 60_000;
const OTHER: &str = "0123456789abcdef0123456789abcdef01234567";

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
        let lamp = Self { root };
        lamp.memory(4 * NEEDS_AVAILABLE_KB);
        lamp
    }
    /// [`MEMINFO`] saying `available_kb` available, as the kernel writes it.
    fn memory(&self, available_kb: u64) {
        self.meminfo(&format!(
            "MemTotal:        8045532 kB\nMemFree:          301234 kB\n\
             MemAvailable:   {available_kb:>8} kB\nBuffers:           12345 kB\n"
        ));
    }
    fn meminfo(&self, text: &str) {
        let meminfo = self.at(MEMINFO);
        fs::create_dir_all(meminfo.parent().unwrap()).unwrap();
        fs::write(meminfo, text).unwrap();
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
    /// Hermes in its folder with its checkout at `commit`, as an installer
    /// leaves it.
    fn hermes_at(&self, commit: &str) {
        put_hermes(self.root(), commit);
    }
    /// Hermes, its two entry points and the file its terminal sources, as
    /// this build's install leaves them.
    fn hermes_there(&self) {
        self.hermes_at(COMMIT);
        fs::write(self.at(WRAPPER), wrapper_text(false)).unwrap();
        fs::write(self.at(NS), ns_text(false, false)).unwrap();
        fs::write(self.at(RUN_ENV), RUN_ENV_TEXT).unwrap();
        agent_ns::write_hook_shim(self.root()).unwrap();
        // The build's own lampos SDK beside them (#6507).
        crate::sdk_tree::write(self.root()).unwrap();
    }
    fn said(&self) -> Option<HermesInstall> {
        record(&self.records())
    }
    fn asked(&self) -> Option<Ask> {
        serde_json::from_slice(&fs::read(self.records().join(ASK)).ok()?).ok()
    }
    fn ask(&self) {
        fs::write(self.records().join(ASK), "{\"version\":1}").unwrap();
    }
    fn entry_points(&self) -> Vec<&'static str> {
        [WRAPPER, NS, RUN_ENV]
            .into_iter()
            .filter(|path| self.at(path).exists())
            .collect()
    }
}

fn put_hermes(root: &Path, commit: &str) {
    let hermes = on(root, HERMES);
    fs::create_dir_all(hermes.parent().unwrap()).unwrap();
    fs::write(hermes, "").unwrap();
    let git = on(root, &format!("{CHECKOUT}/.git"));
    fs::create_dir_all(&git).unwrap();
    fs::write(git.join("HEAD"), format!("{commit}\n")).unwrap();
}

/// The test's own ids, which stand in for `lampos`'s: the folders it makes
/// are its own, as `lampos`'s are on a lamp.
fn own_ids() -> (String, String) {
    let mine = tempfile::tempfile().unwrap().metadata().unwrap();
    (mine.uid().to_string(), mine.gid().to_string())
}

/// A runner that logs each command and answers it: `id -u`/`id -g` with
/// the test's own ids, Hermes's installer stages with `stage` (its
/// `products` stage puts Hermes there with its checkout at `head`),
/// `command -v` with `tools`, the namespace try with `userns`, `df` with
/// `df`, `hermes config set` with `config`, `install -d` by making the
/// folder; the rest with nothing.
#[derive(Clone)]
struct Runs {
    root: PathBuf,
    log: Rc<RefCell<Vec<String>>>,
    stage: Result<String, String>,
    /// A stage that fails alone, with `stage`'s error.
    failing: Option<&'static str>,
    head: String,
    tools: Result<String, String>,
    userns: Result<String, String>,
    config: Result<String, String>,
    /// `df -Pk /var/lib`'s answer.
    df: Result<String, String>,
}

impl Runs {
    fn new(lamp: &Lamp) -> Self {
        Self {
            root: lamp.root().to_owned(),
            log: Rc::default(),
            stage: Ok(String::new()),
            failing: None,
            head: COMMIT.to_owned(),
            tools: Ok("/usr/bin/git\n".into()),
            userns: Ok(String::new()),
            config: Ok(String::new()),
            df: Ok(df(20_000_000)),
        }
    }
    fn runner(&self) -> impl FnMut(&[String]) -> Result<String, String> + '_ {
        move |argv: &[String]| {
            let line = argv.join(" ");
            self.log.borrow_mut().push(line.clone());
            let (uid, gid) = own_ids();
            if line == "id -g lampos" {
                Ok(format!("{gid}\n"))
            } else if argv[0] == "id" {
                Ok(format!("{uid}\n"))
            } else if line.contains(INSTALLER_FILE) {
                let answer = match self.failing {
                    Some(stage) if !line.contains(&format!("--stage {stage} ")) => {
                        Ok(String::new())
                    }
                    _ => self.stage.clone(),
                };
                if answer.is_ok() && line.contains("--stage products ") {
                    put_hermes(&self.root, &self.head);
                }
                answer
            } else if line.contains("command -v") {
                self.tools.clone()
            } else if line.contains("--map-root-user") {
                self.userns.clone()
            } else if argv[0] == "df" {
                self.df.clone()
            } else if line.contains(" config set ") {
                self.config.clone()
            } else if line.ends_with(" --version") {
                Ok("Hermes Agent v0.22.2\n".into())
            } else if argv[0] == "install" && argv[1] == "-d" {
                fs::create_dir_all(on(&self.root, argv.last().unwrap())).unwrap();
                Ok(String::new())
            } else if line.ends_with("harness version") {
                Ok("harness 0.4.25\n".into())
            } else {
                Ok(String::new())
            }
        }
    }
    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
    fn stages(&self) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter(|line| line.contains(INSTALLER_FILE))
            .collect()
    }
    fn ran(&self, start: &str) -> bool {
        self.lines().iter().any(|line| line.starts_with(start))
    }
}

fn df(available_kb: u64) -> String {
    format!(
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
         /dev/mmcblk0p2 60000000 1000 {available_kb} 3% /\n"
    )
}

fn quiet() -> impl FnMut(&str) {
    |_: &str| {}
}

fn install(lamp: &Lamp, runs: &Runs, at: u64) -> Option<HermesInstall> {
    let mut run = runs.runner();
    install_now(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || at,
        &mut quiet(),
    )
    .unwrap()
}

/// A round in an open window: the quiet hour at night.
fn round(lamp: &Lamp, systemd: &mut Fake, at: u64) -> Option<HermesInstall> {
    after_round(
        lamp.root(),
        &lamp.records(),
        OFF,
        systemd,
        at,
        Ok(crate::hermes_schedule::Window::Reached),
        &mut quiet(),
    )
}

/// A round whose window is closed, for `why`.
fn closed_round(lamp: &Lamp, systemd: &mut Fake, at: u64, why: &str) -> Option<HermesInstall> {
    after_round(
        lamp.root(),
        &lamp.records(),
        OFF,
        systemd,
        at,
        Err(why.to_owned()),
        &mut quiet(),
    )
}

/// A round in an open window with the skills switch on (#6128).
fn round_with_skills(lamp: &Lamp, systemd: &mut Fake, at: u64) -> Option<HermesInstall> {
    after_round(
        lamp.root(),
        &lamp.records(),
        SKILLS_ON,
        systemd,
        at,
        Ok(crate::hermes_schedule::Window::Reached),
        &mut quiet(),
    )
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().mode() & 0o7777
}

const AS_LAMPOS: &str = "runuser -u lampos -- env HOME=/var/lib/lampos-hermes \
     HERMES_HOME=/var/lib/lampos-hermes/.hermes PATH=/usr/bin:/bin SHELL=/bin/sh ";

/// On by default on a product lamp, the product owner's decision (10
/// Nov), under the first builder's schedule: unset on a product lamp is
/// the build's default, on; unset on any other lamp is off; set, only
/// `on` is on, so `off` turns a product lamp off. `serve` with it off
/// takes an earlier record and ask away.
#[test]
fn the_switch_is_on_by_default_on_a_product_lamp_only() {
    const { assert!(PRODUCT_DEFAULT) };
    assert!(wanted(None, true, PRODUCT_DEFAULT), "a product lamp, unset");
    assert!(!wanted(None, false, PRODUCT_DEFAULT), "another lamp, unset");
    assert!(!wanted(Some("off"), true, PRODUCT_DEFAULT), "off wins");
    assert!(
        !wanted(None, true, false),
        "a default of off: a product lamp"
    );
    for value in ["off", "", "ON", "1"] {
        for product in [true, false] {
            assert!(!wanted(Some(value), product, true), "{value:?}");
        }
    }
    assert!(wanted(Some("on"), false, PRODUCT_DEFAULT));
    assert!(wanted(Some("on"), true, PRODUCT_DEFAULT));
    let lamp = Lamp::new();
    at_start(lamp.root(), &lamp.records(), false, OFF, 1);
    assert_eq!(fs::read_dir(lamp.records()).unwrap().count(), 0);
    at_start(lamp.root(), &lamp.records(), true, OFF, 1);
    assert_eq!(lamp.said().unwrap().install, "absent");
    lamp.ask();
    at_start(lamp.root(), &lamp.records(), false, OFF, 2);
    assert_eq!(lamp.said(), None);
    assert_eq!(lamp.asked(), None);
    // No ask, no Hermes: a one-shot run for Harness alone runs nothing of it.
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 3), None);
    assert!(runs.lines().is_empty());
    // And off with nothing of Hermes's there, a round starts nothing.
    let mut systemd = Fake::default();
    assert!(!after_round_off(
        lamp.root(),
        &lamp.records(),
        &mut systemd,
        &mut quiet()
    ));
    assert!(systemd.asked.is_empty());
}

/// With Harness there, a round asks and starts Harness's one-shot without
/// waiting, `installing`; a round soon after starts no second. The
/// one-shot looks for the tools, tries the namespace as `lampos`, checks
/// the space, makes Hermes's folder `lampos`'s 0700, runs the checked
/// installer as `lampos` stage by stage with the pinned commit, sets
/// Hermes's config, runs it once, and only then writes the namespace
/// script and the wrapper, root's, 0755; lamp-tasks is started again if it
/// runs. No user, group, sudo or service is made.
#[test]
fn on_with_harness_there_installs_the_pinned_hermes_as_lampos() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    let said = round(&lamp, &mut systemd, 10).unwrap();
    assert_eq!(said.install, "installing");
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
    assert_eq!(lamp.asked(), Some(Ask::default_with(1)));
    assert_eq!(round(&lamp, &mut systemd, 11), None);
    assert_eq!(systemd.asked.len(), 3, "no second start");

    let runs = Runs::new(&lamp);
    let said = install(&lamp, &runs, 20).unwrap();
    assert_eq!(said.install, "installed", "{said:?}");
    assert_eq!(said.version.as_deref(), Some(VERSION));
    assert_eq!(said.commit.as_deref(), Some(COMMIT));
    assert_eq!(lamp.said(), Some(said));
    assert_eq!(lamp.asked(), None, "answered");
    assert!(present(lamp.root(), OFF));

    let lines = runs.lines();
    let at = |wanted: &str| {
        lines
            .iter()
            .position(|line| line == wanted)
            .unwrap_or_else(|| panic!("{wanted} not run: {lines:#?}"))
    };
    let (uid, gid) = own_ids();
    let git = at(&format!("{AS_LAMPOS}sh -c command -v git"));
    at(&format!("{AS_LAMPOS}sh -c command -v curl"));
    at(&format!("{AS_LAMPOS}sh -c command -v bash"));
    at("test -x /usr/bin/unshare");
    let userns = at(&format!(
        "runuser -u lampos -- /usr/bin/unshare --map-root-user --mount --pid --kill-child --mount-proc --propagation private \
         /usr/bin/unshare --user --map-user={uid} --map-group={gid} true"
    ));
    let df = at("df -Pk /var/lib");
    let folder = at("install -d -o lampos -g lampos -m 0700 /var/lib/lampos-hermes");
    let first_stage = lines
        .iter()
        .position(|line| line.contains(INSTALLER_FILE))
        .unwrap();
    assert!(git < userns && userns < df && df < folder && folder < first_stage);
    let stages = runs.stages();
    assert_eq!(stages.len(), STAGES.len());
    for (line, stage) in stages.iter().zip(STAGES) {
        assert!(line.starts_with(&format!("{AS_LAMPOS}bash /")), "{line}");
        assert!(
            line.ends_with(&format!(
                "/{INSTALLER_FILE} --stage {stage} --non-interactive --skip-browser \
                 --skip-computer-use --branch main --commit {COMMIT}"
            )),
            "{line}"
        );
        assert!(install::runs_installer(
            &line.split(' ').map(str::to_owned).collect::<Vec<_>>()
        ));
    }
    let mut last = first_stage;
    for (key, value) in CONFIG {
        let set = at(&format!("{AS_LAMPOS}{HERMES} config set {key} {value}"));
        assert!(set > last);
        last = set;
    }
    let runs_once = at(&format!("{AS_LAMPOS}{HERMES} --version"));
    assert!(runs_once > last);
    assert!(at("systemctl try-restart lamp-tasks.service") > runs_once);
    for line in &lines {
        for no in [
            "useradd", "usermod", "groupadd", "sudo", "visudo", "apt", "gateway",
        ] {
            assert!(!line.contains(no), "{line}");
        }
        if line.contains(INSTALLER_FILE) || line.contains(HERMES) {
            assert!(line.starts_with(AS_LAMPOS), "as lampos: {line}");
        }
    }
    // What lampos reaches Hermes by: root's (the test's own user here),
    // each with its mode and text, and nothing left beside.
    for (path, text) in [
        (WRAPPER, wrapper_text(false)),
        (NS, ns_text(false, false)),
        (agent_ns::HOOK_SHIM, agent_ns::HOOK_SHIM_TEXT.to_owned()),
    ] {
        let place = lamp.at(path);
        assert_eq!(fs::read_to_string(&place).unwrap(), text, "{path}");
        assert_eq!(mode(&place), 0o755, "{path}");
    }
    assert_eq!(mode(&lamp.at(agent_ns::BIN)), 0o755);
    let names = |folder: &str| {
        let mut names: Vec<String> = fs::read_dir(lamp.at(folder))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    // The namespace script, the shared hook stand-in's folder (#6530) and
    // the build's own lampos SDK (#6507, one folder, no staging left beside)
    // and the file Hermes's terminal sources for a run's SDK path (#6684).
    assert_eq!(
        names("/usr/local/lib/lampos"),
        ["hermes-ns", "hermes-run-env.sh", "ns-bin", "sdk-python"]
    );
    assert_eq!(names(agent_ns::BIN), ["harness"]);
    assert!(crate::sdk_tree::present(lamp.root()));
}

impl Ask {
    fn default_with(version: u32) -> Self {
        Self {
            version,
            remove: false,
            skills: false,
            tasks: false,
            one_path: false,
        }
    }
}

/// The wrapper: the token read first and Hermes never started without
/// it; then only `unshare` into the namespace script, with the token as
/// Hermes's key variable, `HERMES_HOME`, and the caller's own ids.
#[test]
fn the_wrapper_fails_closed_without_the_token_and_runs_only_in_the_namespace() {
    assert!(WRAPPER_TEXT.starts_with("#!/bin/sh\n"));
    assert!(WRAPPER_TEXT.contains(&format!("$(cat {TOKEN} 2>/dev/null)")));
    assert!(WRAPPER_TEXT.contains(&format!(
        "export {TOKEN_ENV}=\"$T\" HERMES_HOME={HERMES_HOME} "
    )));
    assert!(WRAPPER_TEXT.ends_with(&format!(
        "exec /usr/bin/unshare --map-root-user --mount --pid --kill-child --mount-proc \
         --propagation private {NS} \"$@\"\n"
    )));
    assert_eq!(
        WRAPPER_TEXT.matches("exec ").count(),
        1,
        "one way to run Hermes"
    );
    assert!(!WRAPPER_TEXT.contains(HERMES), "never Hermes itself");
    let folder = tempfile::tempdir().unwrap();
    let printer = folder.path().join("unshare");
    fs::write(
        &printer,
        "#!/bin/sh\necho \"token=$LAMP_HERMES_TOKEN home=$HERMES_HOME ids=$LAMP_NS_UID:$LAMP_NS_GID\"\necho \"args: $*\"\n",
    )
    .unwrap();
    fs::set_permissions(&printer, fs::Permissions::from_mode(0o755)).unwrap();
    let token = folder.path().join("hermes-token");
    let wrapper = folder.path().join("hermes");
    fs::write(
        &wrapper,
        WRAPPER_TEXT
            .replace(TOKEN, &token.to_string_lossy())
            .replace("/usr/bin/unshare", &printer.to_string_lossy()),
    )
    .unwrap();
    let ran = || {
        std::process::Command::new("/bin/sh")
            .arg(&wrapper)
            .args(["chat", "-q", "a b"])
            .output()
            .unwrap()
    };
    let refused = ran();
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty(), "nothing run");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("no forwarder token"));
    fs::write(&token, "").unwrap();
    assert!(!ran().status.success(), "an empty token is none");
    fs::write(&token, "tok_0123456789abcdef0123456789abcdef\n").unwrap();
    let good = ran();
    assert!(good.status.success(), "{good:?}");
    let out = String::from_utf8_lossy(&good.stdout);
    let (uid, gid) = own_ids();
    assert!(out.contains(&format!(
        "token=tok_0123456789abcdef0123456789abcdef home={HERMES_HOME} ids={uid}:{gid}\n"
    )));
    assert!(out.contains(&format!(
        "args: --map-root-user --mount --pid --kill-child --mount-proc --propagation private \
         {NS} chat -q a b\n"
    )));
}

/// Hermes shares `lampos`'s uid with lamp-cloud, so with the lamp's
/// `/proc` it could open lamp-cloud's `/proc/<pid>/root` (its credentials
/// there), `environ` and `fd` (Thirteen's P1 on #6093). The wrapper gives
/// it a PID namespace of its own (`--pid`), forks into it with the wrapper
/// its parent (`--kill-child`: killing the wrapper kills Hermes) and
/// mounts that namespace's own `/proc` (`--mount-proc`), so no lamp
/// process is visible. On lamp-c0a4 on 10 Nov the lamp-cloud pid did not
/// exist there and three processes were visible.
#[test]
fn the_wrapper_gives_hermes_its_own_pid_namespace_and_proc() {
    let exec = WRAPPER_TEXT
        .lines()
        .find(|line| line.starts_with("exec "))
        .unwrap();
    let words: Vec<&str> = exec.split_whitespace().collect();
    let ns = words.iter().position(|word| *word == NS).unwrap();
    for flag in ["--pid", "--kill-child", "--mount-proc", "--mount"] {
        let at = words.iter().position(|word| *word == flag);
        assert!(at.is_some_and(|at| at < ns), "{flag} before {NS}");
    }
}

/// The namespace script refuses to run in the lamp's own user namespace
/// (or anywhere it cannot read its map), names the allow-list it shows,
/// and drops back to the caller's ids in a nested namespace.
#[test]
fn the_namespace_script_shows_only_what_is_allowed_and_refuses_outside_its_namespace() {
    assert!(NS_TEXT.starts_with("#!/bin/sh\n"));
    assert!(NS_TEXT.contains(&format!(
        "H={PLACE} W={WORK} K={SKILLS} S=/tmp/.lampos-ns\n"
    )));
    assert_eq!(ALLOWED, [PLACE, WORK, SKILLS]);
    assert!(
        HERMES.starts_with(&format!("{PLACE}/")) && HERMES_HOME.starts_with(&format!("{PLACE}/"))
    );
    let hermes = NS_TEXT.find(&format!("{HERMES} \"$@\"")).unwrap();
    for over in OVERLAID {
        let tmpfs = NS_TEXT
            .find(&format!(",nosuid,nodev,noexec none {over} ||"))
            .unwrap_or_else(|| panic!("{over} not overlaid"));
        let ro = NS_TEXT
            .find(&format!(
                "mount -o remount,bind,ro,nosuid,nodev,noexec {over} ||"
            ))
            .unwrap_or_else(|| panic!("{over} not read-only"));
        assert!(tmpfs < ro && ro < hermes, "{over}");
    }
    assert!(
        TOKEN.starts_with("/var/lib/") && !ALLOWED.iter().any(|a| TOKEN.starts_with(a)),
        "the token's own file is hidden too"
    );
    assert!(NS_TEXT.contains("exec /usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" "));
    // The caller's folder, when it is hidden (a hand call from a login
    // home), gives way to Hermes's own with one line, not a bare cd error.
    let back = NS_TEXT
        .find("cd \"$D\" 2>/dev/null || { echo \"hermes-ns: $D is not visible to Hermes; it starts in $H\" >&2; cd \"$H\" 2>/dev/null; } || fail \"neither $D nor $H is visible to Hermes\"")
        .expect("the cd back gives way to Hermes's folder");
    assert!(back < hermes);
    assert!(NS_TEXT.contains("[ \"$count\" != 4294967295 ] ||"));
    // Outside a namespace (and on a host with no /proc), nothing runs.
    let folder = tempfile::tempdir().unwrap();
    let printer = folder.path().join("unshare");
    fs::write(&printer, "#!/bin/sh\necho ran\n").unwrap();
    fs::set_permissions(&printer, fs::Permissions::from_mode(0o755)).unwrap();
    let script = folder.path().join("hermes-ns");
    fs::write(
        &script,
        NS_TEXT
            .replace("/usr/bin/unshare", &printer.to_string_lossy())
            .replace("mount ", "echo mount "),
    )
    .unwrap();
    let out = std::process::Command::new("/bin/sh")
        .arg(&script)
        .env("LAMP_NS_UID", "990")
        .env("LAMP_NS_GID", "990")
        .output()
        .unwrap();
    if !Path::new("/proc/self/uid_map").exists() {
        assert!(!out.status.success());
        assert!(out.stdout.is_empty(), "{out:?}");
    } else if fs::read_to_string("/proc/self/uid_map")
        .unwrap()
        .contains("4294967295")
    {
        assert!(!out.status.success());
        assert!(!String::from_utf8_lossy(&out.stdout).contains("ran"));
    }
}

/// `lampos`'s groups (video, audio, dialout, i2c) can't be dropped under
/// `--map-root-user`, so each device they open is covered by `/dev/null`:
/// the camera, the mic, the servo and serial ports, I2C, SPI and GPIO.
/// `/tmp`, `/var/tmp` and `/dev/shm` are shared on the lamp: Hermes gets
/// its own, before anything is staged there.
#[test]
fn the_namespace_script_hides_the_devices_and_gives_hermes_its_own_tmp() {
    let hermes = NS_TEXT.find(&format!("{HERMES} \"$@\"")).unwrap();
    let devices = NS_TEXT
        .lines()
        .find(|line| line.starts_with("for d in /dev/"))
        .unwrap();
    assert_eq!(devices, format!("for d in {}; do", DEVICES.join(" ")));
    for wanted in [
        "/dev/video*",
        "/dev/media*",
        "/dev/snd/*",
        "/dev/ttyACM*",
        "/dev/ttyUSB*",
        "/dev/i2c-*",
        "/dev/spidev*",
        "/dev/gpiochip*",
    ] {
        assert!(DEVICES.contains(&wanted), "{wanted}");
    }
    let bind = NS_TEXT
        .find("  [ -c \"$d\" ] || continue\n  mount --bind /dev/null \"$d\" || fail \"$d not hidden\"\ndone\n")
        .unwrap();
    assert!(bind < hermes);
    assert_eq!(PRIVATE, ["/tmp", "/var/tmp", "/dev/shm"]);
    let staged = NS_TEXT.find("mkdir -m 0700 \"$S\"").unwrap();
    for private in PRIVATE {
        let tmpfs = NS_TEXT
            .find(&format!("mode=1777,nosuid,nodev none {private} ||"))
            .unwrap_or_else(|| panic!("{private} not its own"));
        assert!(tmpfs < staged, "{private}");
    }
}

/// The reviewer's allow-list check (Thirteen's P1 on #6093): every
/// runtime, state and writable folder under `/run` or `/var/lib` of each
/// product unit run as `lampos`, and every folder the lamp's tmpfiles make
/// `lampos`'s, is under one of [`OVERLAID`] and inside none of
/// [`ALLOWED`], so a new service is hidden from Hermes with no change
/// here. A service folder above an allowed one (lamp-tasks' state folder,
/// above [`WORK`]) shows Hermes only the empty read-only folders down to
/// it. Hermes's own folder, its work folder and the skills are shown.
#[test]
fn no_lampos_service_folder_is_visible_to_hermes() {
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let mut folders = Vec::new();
    for entry in fs::read_dir(&product).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "service") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        if !lines.contains(&"User=lampos") {
            continue;
        }
        for line in lines {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            for word in value.split_whitespace() {
                let name = word.split(':').next().unwrap();
                match key {
                    "RuntimeDirectory" => folders.push(format!("/run/{name}")),
                    "StateDirectory" => folders.push(format!("/var/lib/{name}")),
                    "ReadWritePaths" => {
                        let name = name.trim_start_matches(['-', '+']);
                        if name.starts_with("/run/") || name.starts_with("/var/lib/") {
                            folders.push(name.to_owned());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let tmpfiles = fs::read_to_string(product.join("lampos.conf")).unwrap();
    for line in tmpfiles.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() >= 5
            && !line.starts_with('#')
            && (words[3] == "lampos" || words[4] == "lampos")
        {
            folders.push(words[1].to_owned());
        }
    }
    for wanted in [
        "/run/lampos-eyes",
        // Spelled in two: the voice's reset census keeps the faces
        // folder's whole name to the reset.
        concat!("/var/lib/", "lampos-eyes"),
        "/run/lampos-body",
        "/var/lib/lampos-tasks",
        "/var/lib/lampos-cloud",
        "/run/lampos-bus",
        "/run/lampos-photos",
        SKILLS_SOCKET,
    ] {
        assert!(folders.iter().any(|f| f == wanted), "{wanted} not read");
    }
    let under =
        |path: &str, folder: &str| path == folder || path.starts_with(&format!("{folder}/"));
    for folder in &folders {
        assert!(
            OVERLAID.iter().any(|over| under(folder, over)),
            "{folder} is not under an overlay"
        );
        assert!(
            !ALLOWED.iter().any(|allowed| under(folder, allowed)),
            "{folder}, a lampos service's, is visible to Hermes"
        );
    }
    for allowed in ALLOWED {
        assert!(
            OVERLAID.iter().any(|over| under(allowed, over)),
            "{allowed}"
        );
    }
    assert!(!under(TOKEN, PLACE) && !under(TOKEN, WORK) && !under(TOKEN, SKILLS));
    // The skills switch off (the default): the script names no folder
    // under /run but resolv.conf's, so lamp-skills' is hidden as any is.
    assert!(!NS_TEXT.contains(SKILLS_SOCKET));
    // On (#6128): exactly one lamp service folder is shown, lamp-skills'
    // tools folder, and nothing beside or above it under /run.
    let on = ns_text(true, false);
    let mut named: Vec<&String> = folders
        .iter()
        .filter(|folder| on.contains(&format!("={folder} ")))
        .collect();
    named.sort();
    named.dedup();
    assert_eq!(named, [SKILLS_SOCKET], "{named:?}");
    for folder in &folders {
        assert!(
            folder == SKILLS_SOCKET || !under(folder, SKILLS_SOCKET),
            "{folder} is under the skills' socket folder"
        );
        assert!(
            !under(SKILLS_SOCKET, folder) || folder == SKILLS_SOCKET,
            "{folder} is above the skills' socket folder"
        );
    }
    assert!(under(SKILLS_SOCKET, "/run"));
    // The tasks switch on (#6306): no lamp service folder is shown whole,
    // only lamp-control's one socket file.
    let tasks = ns_text(false, true);
    assert!(
        !folders
            .iter()
            .any(|folder| tasks.contains(&format!("={folder} "))),
        "{folders:?}"
    );
    assert!(under(CONTROL_SOCKET, "/run"));
}

/// The namespace script run under `sh` with every mount stubbed, on a
/// stand-in root: it makes Hermes's own `/tmp` first, stages what stays
/// visible there, lays the overlays, moves the staged mounts back (the
/// skills and `resolv.conf` read-only, with the flags they had), makes
/// the overlays read-only, and only then runs Hermes with `HOME` its own
/// folder. Nothing but [`ALLOWED`] and `resolv.conf` is put back. Any
/// step failing ends it with Hermes not run.
#[test]
fn the_namespace_script_stages_the_allowed_and_stops_at_any_failing_step() {
    stages_the_allowed_and_stops_at_any_failing_step(OFF, false);
}

/// A run started with [`NET_ENV`] `run` while [`TASKS_SWITCH`] is off:
/// the script takes no notice, shows no bridge and starts no relay.
#[test]
fn with_the_tasks_switch_off_a_net_run_is_the_script_as_before() {
    stages_the_allowed_and_stops_at_any_failing_step(OFF, true);
}

/// [`TASKS_SWITCH`] on and [`NET_ENV`] `run` (a skill's run, #6306): the
/// script also stages [`MODEL_SOCKET`], the file alone, shows it
/// read-only, brings `lo` up and starts [`RELAY`] `net-relay` as the
/// caller's own ids, waiting for its `ready`, all before Hermes; each of
/// those steps failing, `ip` missing or the bridge missing ends it before
/// Hermes runs.
#[test]
fn with_the_tasks_switch_on_a_net_run_shows_the_bridge_and_starts_the_relay() {
    stages_the_allowed_and_stops_at_any_failing_step(TASKS_ON, true);
    stages_the_allowed_and_stops_at_any_failing_step(
        Shown {
            skills: true,
            tasks: true,
            one_path: false,
        },
        true,
    );
}

/// [`SKILLS_SWITCH`] on (#6128): the same script, run the same way, also
/// stages [`SKILLS_SOCKET`] before `/run` is hidden, moves it back and
/// remounts it read-only with the flags it had, and removes its staging
/// folder; each of those steps failing ends it before Hermes runs. Nothing
/// else under `/run` but `resolv.conf` is put back. With no skills' socket
/// folder (no lamp-skills), it is skipped and Hermes still runs.
#[test]
fn with_the_skills_switch_on_the_namespace_script_shows_the_skills_socket_read_only() {
    stages_the_allowed_and_stops_at_any_failing_step(SKILLS_ON, false);
}

/// [`TASKS_SWITCH`] on (#6306): the same script, run the same way, also
/// stages [`CONTROL_SOCKET`], the file alone, before `/run` is hidden,
/// moves it back and remounts it read-only, and removes its staging file;
/// each of those steps failing ends it before Hermes runs. `admin.sock`
/// beside it is never named. With no control socket, it is skipped and
/// Hermes still runs.
#[test]
fn with_the_tasks_switch_on_the_namespace_script_shows_the_control_socket_alone_read_only() {
    stages_the_allowed_and_stops_at_any_failing_step(TASKS_ON, false);
}

/// Both switches on: both sockets shown, each in its place.
#[test]
fn with_both_switches_on_the_namespace_script_shows_both_sockets_read_only() {
    stages_the_allowed_and_stops_at_any_failing_step(
        Shown {
            skills: true,
            tasks: true,
            one_path: false,
        },
        false,
    );
}

/// [`ns_text`] for `shown` run under `sh`, every mount stubbed, on a
/// stand-in root that has [`SKILLS_SOCKET`] and [`CONTROL_SOCKET`] (a
/// real unix socket, with `admin.sock` beside it) either way: a switch
/// off, nothing of its socket is touched.
///
/// With `net`, it is run with [`NET_ENV`] `run` and [`MODEL_SOCKET`] (a
/// real unix socket) there; `ip` and the relay (through the `unshare`
/// stub, which says `ready`) are stubbed too.
fn stages_the_allowed_and_stops_at_any_failing_step(shown: Shown, net: bool) {
    let Shown { skills, tasks, .. } = shown;
    // The run's own network steps only with both.
    let netted = tasks && net;
    let folder = tempfile::tempdir().unwrap();
    let base = folder.path().to_string_lossy().into_owned();
    let at = |path: &str| format!("{base}/root{path}");
    let stubs = folder.path().join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let log = folder.path().join("log");
    let resolv = at("/run/systemd/resolve/stub-resolv.conf");
    for made in [
        WORK,
        SKILLS,
        "/etc/lampOS",
        "/dev/shm",
        "/run/systemd/resolve",
        SKILLS_SOCKET,
        PUSHED,
    ] {
        fs::create_dir_all(at(made)).unwrap();
    }
    let pushed = at(PUSHED);
    fs::write(&resolv, "nameserver 127.0.0.53\n").unwrap();
    let control = at(CONTROL_SOCKET);
    let control_folder = at("/run/lampos-control");
    fs::create_dir_all(&control_folder).unwrap();
    let _control = std::os::unix::net::UnixListener::bind(&control).unwrap();
    let _admin =
        std::os::unix::net::UnixListener::bind(format!("{control_folder}/admin.sock")).unwrap();
    let bridge = at(MODEL_SOCKET);
    let bridge_folder = at("/run/lampos-tasks");
    fs::create_dir_all(&bridge_folder).unwrap();
    let _bridge = std::os::unix::net::UnixListener::bind(&bridge).unwrap();
    let uid_map = folder.path().join("uid_map");
    fs::write(&uid_map, "         0        996          1\n").unwrap();
    let stub = "#!/bin/sh\necho \"${0##*/} $*\" >> \"$LOG\"\n\
                if [ -n \"$FAIL_ON\" ]; then case \"${0##*/} $*\" in *\"$FAIL_ON\"*) exit 1;; esac; fi\n\
                case ${0##*/} in findmnt) echo rw,nosuid,nodev,relatime;; readlink) echo \"$RESOLV\";; \
                unshare) case \"$*\" in *net-relay*) echo ready;; *) echo \"HOME=$HOME\";; esac;; \
                ip) if [ \"$*\" = \"-o link show\" ]; then echo \"1: lo: <LOOPBACK,UP> mtu 65536\"; \
                [ ! -f \"$LOG.link\" ] || echo \"2: $(cat \"$LOG.link\"): <BROADCAST> mtu 1500\"; fi;; esac\n";
    for name in [
        "mount", "findmnt", "mkdir", "rmdir", "rm", "touch", "readlink", "unshare", "ip",
    ] {
        let path = stubs.join(name);
        fs::write(&path, stub).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // Each path the script names, moved under the stand-in root in one
    // pass (placeholders first, so no root is moved twice).
    let mut text = ns_text(skills, tasks);
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
        .replace("/usr/bin/unshare", &stubs.join("unshare").to_string_lossy())
        .replace("/usr/sbin/ip /sbin/ip", &stubs.join("ip").to_string_lossy());
    let script = folder.path().join("hermes-ns");
    fs::write(&script, text).unwrap();
    let run = |fail_on: &str| {
        let _ = fs::remove_file(&log);
        let out = std::process::Command::new("/bin/sh")
            .arg(&script)
            .args(["chat", "-q", "a b"])
            .current_dir(at(WORK))
            .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
            .env("LOG", &log)
            .env("FAIL_ON", fail_on)
            .env("RESOLV", &resolv)
            .env("HOME", "/somewhere-else")
            .env("LAMP_NS_UID", "996")
            .env("LAMP_NS_GID", "994")
            .env(NET_ENV, if net { "run" } else { "" })
            .output()
            .unwrap();
        let lines: Vec<String> = fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        (out, lines)
    };
    let (out, lines) = run("");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("HOME={}\n", at(PLACE)),
        "Hermes's HOME is its own folder"
    );
    let find = |wanted: &str| {
        lines
            .iter()
            .position(|line| line == wanted)
            .unwrap_or_else(|| panic!("no `{wanted}` in {lines:#?}"))
    };
    let stage = at("/tmp/.lampos-ns");
    let socket = at(SKILLS_SOCKET);
    // With the switch on, the skills' socket folder's own steps, in their
    // place; off, none.
    let when = |line: String| skills.then_some(line);
    let when_tasks = |line: String| tasks.then_some(line);
    let when_net = |line: String| netted.then_some(line);
    let order = [
        Some(format!(
            "mount -t tmpfs -o size=256m,mode=1777,nosuid,nodev none {}",
            at("/tmp")
        )),
        Some(if skills {
            format!("mkdir -m 0700 {stage} {stage}/h {stage}/w {stage}/k {stage}/c")
        } else {
            format!("mkdir -m 0700 {stage} {stage}/h {stage}/w {stage}/k")
        }),
        Some(format!("mount --bind {} {stage}/h", at(PLACE))),
        Some(format!("mount --bind {} {stage}/w", at(WORK))),
        Some(format!("mount --bind {} {stage}/k", at(SKILLS))),
        when(format!("mount --bind {socket} {stage}/c")),
        Some(format!("mount --bind {resolv} {stage}/r")),
        when_tasks(format!("touch {stage}/l")),
        when_tasks(format!("mount --bind {control} {stage}/l")),
        when_net(format!("touch {stage}/b")),
        when_net(format!("mount --bind {bridge} {stage}/b")),
        when_net(format!("mkdir -m 0700 {stage}/p")),
        when_net(format!("mount --bind {pushed} {stage}/p")),
        Some(format!(
            "mount -t tmpfs -o size=1m,mode=0755,nosuid,nodev,noexec none {}",
            at("/run")
        )),
        Some(format!(
            "mount -t tmpfs -o size=1m,mode=0755,nosuid,nodev,noexec none {}",
            at("/var/lib")
        )),
        Some(format!(
            "mount -t tmpfs -o size=4k,mode=0555,nosuid,nodev,noexec none {}",
            at("/etc/lampOS")
        )),
        Some(format!("mount --move {stage}/h {}", at(PLACE))),
        Some(format!("mount --move {stage}/w {}", at(WORK))),
        Some(format!("mount --move {stage}/k {}", at(SKILLS))),
        Some(format!(
            "mount -o remount,bind,ro,nosuid,nodev,relatime {}",
            at(SKILLS)
        )),
        when(format!("mount --move {stage}/c {socket}")),
        when(format!(
            "mount -o remount,bind,ro,nosuid,nodev,relatime {socket}"
        )),
        Some(format!("mount --move {stage}/r {resolv}")),
        Some(format!(
            "mount -o remount,bind,ro,nosuid,nodev,relatime {resolv}"
        )),
        when_tasks(format!("mkdir -p {control_folder}")),
        when_tasks(format!("touch {control}")),
        when_tasks(format!("mount --move {stage}/l {control}")),
        when_tasks(format!(
            "mount -o remount,bind,ro,nosuid,nodev,relatime {control}"
        )),
        when_net(format!("mkdir -p {bridge_folder}")),
        when_net(format!("touch {bridge}")),
        when_net(format!("mount --move {stage}/b {bridge}")),
        when_net(format!(
            "mount -o remount,bind,ro,nosuid,nodev,relatime {bridge}"
        )),
        when_net(format!("mkdir -p {pushed}")),
        when_net(format!("mount --move {stage}/p {pushed}")),
        when_net(format!(
            "mount -o remount,bind,ro,nosuid,nodev,relatime {pushed}"
        )),
        Some(if tasks {
            format!("rm -f {stage}/r {stage}/l {stage}/b")
        } else {
            format!("rm -f {stage}/r")
        }),
        when_net(format!("rmdir {stage}/p")),
        Some(if skills {
            format!("rmdir {stage}/h {stage}/w {stage}/k {stage}/c {stage}")
        } else {
            format!("rmdir {stage}/h {stage}/w {stage}/k {stage}")
        }),
        Some(format!(
            "mount -o remount,bind,ro,nosuid,nodev,noexec {}",
            at("/run")
        )),
        Some(format!(
            "mount -o remount,bind,ro,nosuid,nodev,noexec {}",
            at("/var/lib")
        )),
        Some(format!(
            "mount -o remount,bind,ro,nosuid,nodev,noexec {}",
            at("/etc/lampOS")
        )),
        when_net("ip link set lo up".to_owned()),
        when_net(format!(
            "unshare --user --map-user=996 --map-group=994 {RELAY} net-relay \
             --listen {RELAY_LISTEN} --to {bridge}"
        )),
        Some(format!(
            "unshare --user --map-user=996 --map-group=994 {} chat -q a b",
            at(HERMES)
        )),
    ];
    let order: Vec<String> = order.into_iter().flatten().collect();
    let places: Vec<usize> = order.iter().map(|line| find(line)).collect();
    if !skills {
        // Off: the skills' socket folder, there on this root, is never
        // named, so it stays under the empty /run.
        assert!(
            !lines.iter().any(|line| line.contains(&socket)),
            "{lines:#?}"
        );
    }
    if !tasks {
        // Off: nothing of lamp-control's folder, there on this root, is
        // named, so it stays under the empty /run.
        assert!(
            !lines.iter().any(|line| line.contains(&control_folder)),
            "{lines:#?}"
        );
    }
    if !netted {
        // No run of its own network: the bridge, there on this root, is
        // never named, `lo` is left alone and no relay is started.
        assert!(
            !lines
                .iter()
                .any(|line| line.contains(&bridge_folder) || line.starts_with("ip ")),
            "{lines:#?}"
        );
        assert!(!lines.iter().any(|line| line.contains("net-relay")));
        // Nor the pushed skills, there on this root: only a skill's run
        // sees them.
        assert!(
            !lines.iter().any(|line| line.contains(&pushed)),
            "{lines:#?}"
        );
    }
    // Never lamp-control's owner socket, either way.
    assert!(
        !lines.iter().any(|line| line.contains("admin.sock")),
        "{lines:#?}"
    );
    assert!(places.windows(2).all(|two| two[0] < two[1]), "{lines:#?}");
    assert_eq!(*places.last().unwrap(), lines.len() - 1, "Hermes last");
    let shown: Vec<&str> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("mount --move "))
        .map(|line| line.rsplit(' ').next().unwrap())
        .collect();
    let allowed: Vec<String> = ALLOWED
        .iter()
        .map(|path| at(path))
        .chain(skills.then(|| socket.clone()))
        .chain([resolv.clone()])
        .chain(tasks.then(|| control.clone()))
        .chain(netted.then(|| bridge.clone()))
        .chain(netted.then(|| pushed.clone()))
        .collect();
    assert_eq!(
        shown,
        allowed.iter().map(String::as_str).collect::<Vec<_>>()
    );
    // Each step failing ends it before Hermes runs.
    let socket_steps = [
        format!("--bind {socket}"),
        format!("mount --move {stage}/c"),
        format!("findmnt -no VFS-OPTIONS -M {socket}"),
        format!("remount,bind,ro,nosuid,nodev,relatime {socket}"),
    ];
    let steps = [
        format!("none {}", at("/tmp")),
        format!("none {}", at("/var/tmp")),
        "mkdir -m 0700".to_owned(),
        format!("--bind {}", at(PLACE)),
        format!("--bind {}", at(WORK)),
        format!("--bind {}", at(SKILLS)),
        format!("--bind {resolv}"),
        format!("none {}", at("/run")),
        format!("none {}", at("/var/lib")),
        format!("none {}", at("/etc/lampOS")),
        format!("mount --move {stage}/h"),
        format!("mount --move {stage}/w"),
        format!("mount --move {stage}/k"),
        format!("mount --move {stage}/r"),
        "findmnt".to_owned(),
        "remount,bind,ro".to_owned(),
        "rmdir".to_owned(),
        format!("remount,bind,ro,nosuid,nodev,noexec {}", at("/run")),
        format!("remount,bind,ro,nosuid,nodev,noexec {}", at("/var/lib")),
        format!("remount,bind,ro,nosuid,nodev,noexec {}", at("/etc/lampOS")),
    ];
    let control_steps = [
        format!("touch {stage}/l"),
        format!("--bind {control}"),
        format!("mount --move {stage}/l"),
        format!("findmnt -no VFS-OPTIONS -M {control}"),
        format!("remount,bind,ro,nosuid,nodev,relatime {control}"),
    ];
    let net_steps = [
        format!("touch {stage}/b"),
        format!("--bind {bridge}"),
        format!("mount --move {stage}/b"),
        format!("findmnt -no VFS-OPTIONS -M {bridge}"),
        format!("remount,bind,ro,nosuid,nodev,relatime {bridge}"),
        format!("mkdir -m 0700 {stage}/p"),
        format!("--bind {pushed}"),
        format!("mount --move {stage}/p"),
        format!("findmnt -no VFS-OPTIONS -M {pushed}"),
        format!("remount,bind,ro,nosuid,nodev,relatime {pushed}"),
        format!("rmdir {stage}/p"),
        "ip link set lo up".to_owned(),
        "net-relay".to_owned(),
    ];
    let socket_steps = if skills { &socket_steps[..] } else { &[] };
    let control_steps = if tasks { &control_steps[..] } else { &[] };
    let net_steps = if netted { &net_steps[..] } else { &[] };
    for fail_on in steps
        .iter()
        .chain(socket_steps)
        .chain(control_steps)
        .chain(net_steps)
    {
        let (out, lines) = run(fail_on);
        assert!(!out.status.success(), "{fail_on}: {lines:#?}");
        assert!(
            !lines.iter().any(|line| line.ends_with(" chat -q a b")),
            "{fail_on}: Hermes ran"
        );
        assert!(
            lines.iter().any(|line| line.contains(fail_on.as_str())),
            "{fail_on}: never reached"
        );
    }
    if netted {
        // Fail closed: a link besides `lo` in the run's namespace (the
        // network not held off), and Hermes never runs.
        let more = format!("{}.link", log.display());
        fs::write(&more, "eth0").unwrap();
        let (out, lines) = run("");
        fs::remove_file(&more).unwrap();
        assert!(!out.status.success(), "{lines:#?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("a link besides lo"));
        assert!(!lines.iter().any(|line| line.ends_with(" chat -q a b")));
        // The kernel's fallback tunnels (Heron's P3 on #6443): a new
        // namespace gets `sit0@NONE` and the like when their modules are
        // loaded; nothing is under them, so the run goes on.
        fs::write(&more, "sit0@NONE").unwrap();
        let (out, lines) = run("");
        fs::remove_file(&more).unwrap();
        assert!(out.status.success(), "{lines:#?}");
        assert!(lines.iter().any(|line| line.ends_with(" chat -q a b")));
        // Fail closed: no `ip` to bring `lo` up, or no bridge socket (the
        // switch off in lamp-tasks), and Hermes never runs.
        let ip = stubs.join("ip");
        fs::rename(&ip, stubs.join("ip.gone")).unwrap();
        let (out, lines) = run("");
        assert!(!out.status.success(), "{lines:#?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("no ip"));
        assert!(!lines.iter().any(|line| line.ends_with(" chat -q a b")));
        assert!(!lines.iter().any(|line| line.contains("net-relay")));
        fs::rename(stubs.join("ip.gone"), &ip).unwrap();
        fs::remove_file(&bridge).unwrap();
        let (out, lines) = run("");
        assert!(!out.status.success(), "{lines:#?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains(&format!("no {bridge}")));
        assert!(!lines.iter().any(|line| line.ends_with(" chat -q a b")));
        let _bridge = std::os::unix::net::UnixListener::bind(&bridge).unwrap();
        // No pushed skills on this lamp: skipped, and Hermes still runs.
        fs::remove_dir(&pushed).unwrap();
        let (out, lines) = run("");
        assert!(out.status.success(), "{lines:#?}");
        assert!(lines.iter().any(|line| line.ends_with(" chat -q a b")));
        assert!(
            !lines.iter().any(|line| line.contains(&pushed)),
            "{lines:#?}"
        );
        fs::create_dir_all(&pushed).unwrap();
    }
    if tasks {
        // A lamp with no control socket (lamp-control not serving): skipped,
        // and Hermes still runs, with nothing of lamp-control's shown.
        fs::remove_file(&control).unwrap();
        let (out, lines) = run("");
        assert!(out.status.success(), "{out:?}");
        assert!(
            !lines.iter().any(|line| line.contains(&control_folder)),
            "{lines:#?}"
        );
        fs::remove_dir_all(&control_folder).unwrap();
    }
    if !skills {
        return;
    }
    // A lamp with no skills' socket folder (no lamp-skills): skipped, and
    // Hermes still runs; nothing else under /run but resolv.conf is shown.
    fs::remove_dir_all(&socket).unwrap();
    let (out, lines) = run("");
    assert!(out.status.success(), "{out:?}");
    assert!(
        !lines.iter().any(|line| line.contains(&socket)),
        "{lines:#?}"
    );
    let shown: Vec<&str> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("mount --move "))
        .map(|line| line.rsplit(' ').next().unwrap())
        .collect();
    assert_eq!(
        shown,
        [at(PLACE), at(WORK), at(SKILLS), resolv.clone()]
            .iter()
            .chain(netted.then_some(&bridge))
            .chain(netted.then_some(&pushed))
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        lines.last().map(String::as_str),
        Some(
            format!(
                "unshare --user --map-user=996 --map-group=994 {} chat -q a b",
                at(HERMES)
            )
            .as_str()
        )
    );
}

/// Hermes's own update check, its plugins' and its language servers are
/// set off, and a one-shot run ends with its turn.
#[test]
fn hermes_is_set_never_to_update_itself_or_linger() {
    assert_eq!(
        CONFIG,
        [
            ("updates.check", "false"),
            ("plugins.auto_update_check_hours", "0"),
            ("lsp.enabled", "false"),
            ("terminal.oneshot_completion_wait_seconds", "0"),
        ]
    );
    assert!(!STAGES.contains(&"gateway") && !STAGES.contains(&"setup"));
}

/// No user namespace for `lampos`: `no_userns`, and nothing is made or
/// installed, so Hermes never runs where it could read the relay key.
#[test]
fn no_user_namespace_is_no_userns_and_nothing_is_installed() {
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs {
        userns: Err("unshare: unshare failed: Operation not permitted".into()),
        ..Runs::new(&lamp)
    };
    let said = install(&lamp, &runs, 5).unwrap();
    assert_eq!(said.error.as_deref(), Some("no_userns"));
    assert!(said.check());
    assert!(runs.stages().is_empty());
    assert!(!runs.ran("install -d") && !runs.ran("df"));
    assert!(lamp.entry_points().is_empty());
    assert!(!lamp.at(PLACE).exists());
}

/// With Harness missing, the round asks but starts nothing itself: Hermes
/// waits, `absent`. Harness's one-shot then installs Harness first and
/// Hermes right after it, in one run.
#[test]
fn on_with_harness_missing_hermes_is_installed_right_after_harness() {
    let lamp = Lamp::new();
    at_start(lamp.root(), &lamp.records(), true, OFF, 1);
    let mut systemd = Fake::default();
    let harness_said = harness_install::after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        5,
        &mut quiet(),
    )
    .unwrap();
    assert_eq!(harness_said.install, "installing");
    assert_eq!(round(&lamp, &mut systemd, 5), None, "still absent");
    assert_eq!(lamp.said().unwrap().install, "absent");
    assert_eq!(
        systemd.asked,
        ["reload", "show", "start"],
        "Harness's start alone"
    );
    assert!(lamp.asked().is_some());

    let runs = Runs::new(&lamp);
    let mut run = runs.runner();
    one_shot(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 9,
        &mut quiet(),
    )
    .unwrap();
    drop(run);
    let lines = runs.lines();
    let harness = lines
        .iter()
        .position(|line| line.contains(install::HARNESS_INSTALLER))
        .expect("Harness's installer ran");
    let hermes = lines
        .iter()
        .position(|line| line.contains(INSTALLER_FILE))
        .expect("Hermes's installer ran");
    assert!(harness < hermes, "Harness first: {lines:#?}");
    assert_eq!(lamp.said().unwrap().install, "installed");
    assert_eq!(
        harness_install::record(&lamp.records()).unwrap().install,
        "installed"
    );
}

/// Harness's install failing never stops Hermes's, and Hermes's failing
/// never fails the one-shot: the unit's result is Harness's alone. A
/// one-shot started with no Harness ask (its switch off) leaves Harness
/// alone.
#[test]
fn a_failure_of_one_never_blocks_the_other() {
    let lamp = Lamp::new();
    lamp.ask();
    fs::write(
        lamp.records().join(harness_install::ASK),
        "{\"version\":1,\"pin\":null}",
    )
    .unwrap();
    let runs = Runs {
        failing: Some("python-deps"),
        stage: Err("bash: exit status 1".into()),
        ..Runs::new(&lamp)
    };
    let log = runs.log.clone();
    let mut run = move |argv: &[String]| {
        if argv
            .last()
            .is_some_and(|last| last.contains(install::HARNESS_INSTALLER))
        {
            log.borrow_mut().push(argv.join(" "));
            return Err("sh: exit status 1".to_owned());
        }
        runs.runner()(argv)
    };
    let ended = one_shot(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 9,
        &mut quiet(),
    );
    assert_eq!(ended, Err("Harness is not installed: installer".into()));
    let said = lamp.said().unwrap();
    assert_eq!(
        (said.install.as_str(), said.error.as_deref()),
        ("failed", Some("installer"))
    );

    // No Harness ask, Harness missing: only Hermes's part runs, and it
    // failing ends the one-shot well, Harness's record untouched.
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs {
        stage: Err("bash: exit status 1".into()),
        ..Runs::new(&lamp)
    };
    let mut run = runs.runner();
    assert_eq!(
        one_shot(
            lamp.root(),
            &lamp.records(),
            &mut run,
            &mut || 9,
            &mut quiet()
        ),
        Ok(())
    );
    drop(run);
    assert!(
        !runs
            .lines()
            .iter()
            .any(|line| line.contains(install::HARNESS_INSTALLER))
    );
    assert_eq!(lamp.said().unwrap().error.as_deref(), Some("installer"));
    assert_eq!(harness_install::record(&lamp.records()), None);
}

/// The installer's bytes not the pinned ones: nothing runs, `pin_mismatch`.
#[test]
fn an_installer_whose_sha256_differs_is_refused() {
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs::new(&lamp);
    let mut run = runs.runner();
    let kit = Kit {
        installer: b"#!/usr/bin/env bash\necho not the pinned installer\n",
        installer_sha256: INSTALLER_SHA256,
        commit: COMMIT,
    };
    let said = install_with(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 3,
        &mut quiet(),
        &kit,
    )
    .unwrap()
    .unwrap();
    assert_eq!(said.error.as_deref(), Some("pin_mismatch"));
    drop(run);
    assert!(runs.lines().is_empty(), "{:?}", runs.lines());
    assert!(lamp.entry_points().is_empty());
}

/// A stage that fails is `installer` (one past its bound `timeout`), with
/// no later stage run and no entry point written; the next round leaves it
/// for 24 h, then asks and starts the one-shot again, which repairs the
/// same folder (nothing moved aside: the checkout is the pinned one or
/// none).
#[test]
fn a_failure_is_recorded_and_tried_again_a_day_later() {
    let lamp = Lamp::new();
    lamp.harness_there();
    lamp.ask();
    let runs = Runs {
        failing: Some("venv"),
        stage: Err("bash: exit status 1".into()),
        ..Runs::new(&lamp)
    };
    let said = install(&lamp, &runs, 100).unwrap();
    assert_eq!(
        (said.install.as_str(), said.error.as_deref()),
        ("failed", Some("installer"))
    );
    let stages = runs.stages();
    assert_eq!(
        stages.len(),
        3,
        "prerequisites, repository, venv: {stages:#?}"
    );
    assert!(!runs.ran(&format!("{AS_LAMPOS}{HERMES}")));
    assert!(lamp.entry_points().is_empty());
    let mut systemd = Fake::default();
    assert_eq!(round(&lamp, &mut systemd, 100 + 23 * HOUR_MS), None);
    assert!(systemd.asked.is_empty());
    let again = round(&lamp, &mut systemd, 100 + 24 * HOUR_MS).unwrap();
    assert_eq!(again.install, "installing");
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
    // A checkout left at the pin by a cut-off try is repaired in place.
    let git = lamp.at(&format!("{CHECKOUT}/.git"));
    fs::create_dir_all(&git).unwrap();
    fs::write(git.join("HEAD"), format!("{COMMIT}\n")).unwrap();
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 200).unwrap().install, "installed");
    assert!(!lamp.at(ASIDE).exists());
    assert_eq!(runs.stages().len(), STAGES.len());

    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs {
        stage: Err("bash: did not end within 900 s: it ran on".into()),
        ..Runs::new(&lamp)
    };
    assert_eq!(
        install(&lamp, &runs, 200).unwrap().error.as_deref(),
        Some("timeout")
    );
    // The unit not started is tried at the next round.
    let mut refused = Fake {
        start: Some(Err("no".into())),
        ..Fake::default()
    };
    let lamp = Lamp::new();
    lamp.harness_there();
    let said = round(&lamp, &mut refused, 5).unwrap();
    assert_eq!(said.error.as_deref(), Some("not_started"));
    assert_eq!(
        round(&lamp, &mut Fake::default(), 6).unwrap().install,
        "installing"
    );
}

/// Hermes's config not set: `config`, no entry point.
#[test]
fn a_config_not_set_is_config_and_leaves_no_entry_point() {
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs {
        config: Err("hermes: exit status 1".into()),
        ..Runs::new(&lamp)
    };
    assert_eq!(
        install(&lamp, &runs, 5).unwrap().error.as_deref(),
        Some("config")
    );
    assert!(lamp.entry_points().is_empty());
}

/// No folder for the namespace script: `entry`, and no wrapper left.
#[test]
fn no_folder_for_the_namespace_script_is_entry() {
    let lamp = Lamp::new();
    lamp.ask();
    fs::remove_dir(lamp.at("/usr/local/lib/lampos")).unwrap();
    let runs = Runs::new(&lamp);
    assert_eq!(
        install(&lamp, &runs, 5).unwrap().error.as_deref(),
        Some("entry")
    );
    assert!(lamp.entry_points().is_empty(), "{:?}", lamp.entry_points());
}

/// A failed install over one that was there (a wrapper left from before)
/// takes the entry points out; Hermes's folder stays.
#[test]
fn a_failed_install_takes_the_entry_points_out() {
    let lamp = Lamp::new();
    lamp.hermes_there();
    fs::remove_file(lamp.at(HERMES)).unwrap();
    lamp.ask();
    let runs = Runs {
        stage: Err("bash: exit status 1".into()),
        ..Runs::new(&lamp)
    };
    assert_eq!(
        install(&lamp, &runs, 5).unwrap().error.as_deref(),
        Some("installer")
    );
    assert!(lamp.entry_points().is_empty());
    assert!(lamp.at(PLACE).exists(), "the folder stays");
}

/// A Hermes of another commit in its folder (an older build's pin, or one
/// on a branch): moved aside whole before the install and removed once the
/// new one runs. A failed install keeps it aside; a later try with one
/// aside already removes the other instead of moving it there.
#[test]
fn another_hermes_is_moved_aside_then_removed_once_the_new_one_runs() {
    let lamp = Lamp::new();
    lamp.hermes_at(OTHER);
    fs::write(lamp.at(&format!("{HERMES_HOME}/config.yaml")), "old: 1\n").unwrap();
    fs::write(lamp.at(WRAPPER), "#!/bin/sh\nexec old\n").unwrap();
    assert!(!present(lamp.root(), OFF));
    lamp.ask();
    // First a failed try: the old one stays aside.
    let runs = Runs {
        failing: Some("python-deps"),
        stage: Err("bash: exit status 1".into()),
        ..Runs::new(&lamp)
    };
    assert_eq!(
        install(&lamp, &runs, 5).unwrap().error.as_deref(),
        Some("installer")
    );
    assert_eq!(
        fs::read_to_string(lamp.at(&format!("{ASIDE}/.hermes/config.yaml"))).unwrap(),
        "old: 1\n"
    );
    assert!(lamp.entry_points().is_empty(), "the old wrapper is out");
    // An other one again in the folder, with one aside: this one removed.
    lamp.hermes_at(OTHER);
    lamp.ask();
    let runs = Runs::new(&lamp);
    let said = install(&lamp, &runs, 6).unwrap();
    assert_eq!(said.install, "installed", "{said:?}");
    assert!(!lamp.at(ASIDE).exists(), "removed once the new one runs");
    assert_eq!(head(lamp.root()).as_deref(), Some(COMMIT));
    assert!(present(lamp.root(), OFF));
    let lines = runs.lines();
    let ran = lines
        .iter()
        .position(|line| line.ends_with(" --version"))
        .unwrap();
    assert!(
        ran > lines
            .iter()
            .position(|line| line.contains("--stage products"))
            .unwrap()
    );

    // A link at the folder is removed, never followed.
    let lamp = Lamp::new();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::write(elsewhere.path().join("keep"), "").unwrap();
    symlink(elsewhere.path(), lamp.at(PLACE)).unwrap();
    lamp.ask();
    assert_eq!(
        install(&lamp, &Runs::new(&lamp), 7).unwrap().install,
        "installed"
    );
    assert!(elsewhere.path().join("keep").exists());
}

/// Hermes at the pin with an older build's wrapper: no stage runs again;
/// its config is set, it is run once, and the entry points written anew.
/// lamp-tasks is not started again.
#[test]
fn an_older_wrapper_over_the_pinned_hermes_is_written_anew_with_no_install() {
    let lamp = Lamp::new();
    lamp.hermes_there();
    fs::write(lamp.at(WRAPPER), "#!/bin/sh\nexec sudo old\n").unwrap();
    assert!(!present(lamp.root(), OFF));
    lamp.ask();
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 5).unwrap().install, "installed");
    assert!(runs.stages().is_empty());
    assert!(!runs.ran("df") && !runs.ran("install -d") && !runs.ran("systemctl"));
    assert!(runs.ran(&format!(
        "{AS_LAMPOS}{HERMES} config set updates.check false"
    )));
    assert!(present(lamp.root(), OFF));
}

/// The switch off: a round that finds an entry point asks the one-shot to
/// take them out, and the one-shot does, record and all; Hermes's folder
/// stays, and nothing is run.
#[test]
fn the_switch_off_takes_the_wrapper_out() {
    let lamp = Lamp::new();
    lamp.hermes_there();
    let mut systemd = Fake::default();
    assert!(after_round_off(
        lamp.root(),
        &lamp.records(),
        &mut systemd,
        &mut quiet()
    ));
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
    assert_eq!(
        lamp.asked(),
        Some(Ask {
            version: 1,
            remove: true,
            skills: false,
            tasks: false,
            one_path: false,
        })
    );
    let runs = Runs::new(&lamp);
    let mut run = runs.runner();
    one_shot(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 9,
        &mut quiet(),
    )
    .unwrap();
    drop(run);
    assert!(runs.lines().is_empty(), "{:?}", runs.lines());
    assert!(lamp.entry_points().is_empty());
    assert!(
        !lamp.at(agent_ns::BIN).exists(),
        "the hook stand-in goes with the last namespace script"
    );
    assert!(lamp.at(HERMES).exists(), "Hermes in its folder stays");
    assert_eq!(lamp.asked(), None);
    assert_eq!(lamp.said(), None);
    // Nothing left: the next round starts nothing.
    assert!(!after_round_off(
        lamp.root(),
        &lamp.records(),
        &mut systemd,
        &mut quiet()
    ));
    assert_eq!(systemd.asked.len(), 3);
}

/// No `git`, `curl` or `bash` on the lamp: `no_tools`, and nothing is
/// added to the system for it.
#[test]
fn no_git_is_said_and_nothing_is_installed() {
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs {
        tools: Err("sh: exit status 1".into()),
        ..Runs::new(&lamp)
    };
    assert_eq!(
        install(&lamp, &runs, 5).unwrap().error.as_deref(),
        Some("no_tools")
    );
    assert!(runs.stages().is_empty());
    assert!(!runs.ran("apt"));
}

/// The checkout at another commit after the installer: `version`, no
/// entry point; a round 24 h later tries again.
#[test]
fn a_checkout_at_another_commit_is_version() {
    let lamp = Lamp::new();
    lamp.harness_there();
    lamp.ask();
    let runs = Runs {
        head: OTHER.to_owned(),
        ..Runs::new(&lamp)
    };
    assert_eq!(
        install(&lamp, &runs, 5).unwrap().error.as_deref(),
        Some("version")
    );
    assert!(lamp.entry_points().is_empty());
    assert!(!present(lamp.root(), OFF));
    assert_eq!(round(&lamp, &mut Fake::default(), 6), None);
    assert_eq!(
        round(&lamp, &mut Fake::default(), 5 + 24 * HOUR_MS)
            .unwrap()
            .install,
        "installing"
    );
}

/// Hermes already there is left alone: recorded `installed` once, no ask,
/// no unit started, and a one-shot that finds it there runs nothing.
#[test]
fn hermes_already_there_is_left_alone() {
    let lamp = Lamp::new();
    lamp.harness_there();
    lamp.hermes_there();
    let mut systemd = Fake::default();
    let said = round(&lamp, &mut systemd, 5).unwrap();
    assert_eq!(
        (said.install.as_str(), said.commit.as_deref()),
        ("installed", Some(COMMIT))
    );
    assert_eq!(round(&lamp, &mut systemd, 6), None);
    assert!(systemd.asked.is_empty());
    assert_eq!(lamp.asked(), None);
    lamp.ask();
    let runs = Runs::new(&lamp);
    let said = install(&lamp, &runs, 7).unwrap();
    assert_eq!(said.install, "installed");
    assert!(runs.lines().is_empty());
    assert_eq!(
        lamp.said().unwrap().revision,
        1,
        "the record kept as it was"
    );
}

/// The compiled-in installer is the one its constants name, and it takes
/// what lamp-update passes: `--commit`, `--branch`, each stage by name,
/// the skips, and `HERMES_HOME`.
#[test]
fn the_built_in_installer_is_the_pinned_one_and_takes_the_pin() {
    assert_eq!(crate::manifest::sha256_hex(INSTALLER), INSTALLER_SHA256);
    let text = std::str::from_utf8(INSTALLER).unwrap();
    assert!(text.starts_with("#!/usr/bin/env bash\n"));
    assert!(text.contains("--commit|-Commit) INSTALL_COMMIT=\"$2\" ;;"));
    assert!(text.contains("HERMES_HOME=\"${HERMES_HOME:-$HOME/.hermes}\""));
    for flag in ["--skip-browser", "--skip-computer-use", "--non-interactive"] {
        assert!(text.contains(&format!("{flag}|")), "{flag}");
    }
    for stage in STAGES {
        assert!(
            text.contains(&format!("        {stage}) stage_")),
            "{stage}"
        );
    }
    assert!(COMMIT.len() == 40 && COMMIT.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(COMMIT.starts_with("d923a5eb487a"));
    assert_eq!(VERSION, "0.22.2");
    assert!(INSTALLER_ORIGIN.contains(&format!("/v{VERSION}/")));
    assert_eq!(format!("{PLACE}/.hermes"), HERMES_HOME);
    assert_eq!(format!("{HERMES_HOME}/hermes-agent"), CHECKOUT);
    assert_eq!(format!("{PLACE}/.local/bin/hermes"), HERMES);
}

/// One stage of Hermes's installer gets the installer's bound, not a step's.
#[test]
fn a_hermes_stage_gets_the_installers_bound() {
    let argv = |line: &str| line.split(' ').map(str::to_owned).collect::<Vec<_>>();
    assert!(install::runs_installer(&argv(
        "runuser -u lampos -- env bash /tmp/x/hermes-install.sh --stage venv"
    )));
    assert!(!install::runs_installer(&argv(
        "runuser -u lampos -- env /var/lib/lampos-hermes/.local/bin/hermes --version"
    )));
    assert!(!install::runs_installer(&argv(
        "cat /tmp/x/hermes-install.sh"
    )));
}

/// The one-shot's unit may write `/usr/local/bin` and, when it is there,
/// `/usr/local/lib/lampos`; no more of the system, `/etc` not at all.
#[test]
fn the_one_shot_may_write_only_where_the_entry_points_go() {
    let lines: Vec<&str> = harness_install::TEXT.lines().collect();
    assert!(lines.contains(&"ReadWritePaths=/usr/local/bin -/usr/local/lib/lampos"));
    assert!(lines.contains(&"ProtectSystem=full"));
    assert!(lines.contains(&"TimeoutStartSec=3600"));
    assert!(!harness_install::TEXT.contains(" /etc\n"));
    assert_eq!(
        Path::new(WRAPPER).parent(),
        Some(Path::new("/usr/local/bin"))
    );
    assert_eq!(
        Path::new(NS).parent(),
        Some(Path::new("/usr/local/lib/lampos"))
    );
}

/// Under 4 GiB free in `/var/lib` (Hermes took 3.0 GB on lamp-c0a4), or
/// `df` not read: `no_space`, and no folder or stage made. Exactly enough
/// is enough.
#[test]
fn too_little_free_space_is_no_space_and_nothing_is_made() {
    for answer in [
        Ok(df(NEEDS_FREE_KB - 1)),
        Ok("df: no\n".into()),
        Err("df: exit 1".into()),
    ] {
        let lamp = Lamp::new();
        lamp.ask();
        let runs = Runs {
            df: answer.clone(),
            ..Runs::new(&lamp)
        };
        let said = install(&lamp, &runs, 5).unwrap();
        assert_eq!(said.error.as_deref(), Some("no_space"), "{answer:?}");
        assert!(lamp.said().unwrap().check());
        assert!(runs.ran("df -Pk /var/lib"));
        assert!(!runs.ran("install -d") && runs.stages().is_empty());
        assert!(lamp.entry_points().is_empty());
    }
    let lamp = Lamp::new();
    lamp.ask();
    let runs = Runs {
        df: Ok(df(NEEDS_FREE_KB)),
        ..Runs::new(&lamp)
    };
    assert_eq!(install(&lamp, &runs, 5).unwrap().install, "installed");
}

/// The product units turn the lamp's Hermes on for voice work (#5978, the
/// first builder's (a), 11 Nov): lamp-tasks and lamp-cloud each set the
/// switch, and lamp-tasks has `--engines-from-machines`, without which the
/// lamp's own Hermes is never looked for. lamp-update's own unit still
/// decides the install by its `PRODUCT_DEFAULT`, with the schedule; neither
/// unit names Hermes's folder.
#[test]
fn the_product_units_turn_the_lamps_hermes_on_for_voice_work() {
    let tasks = include_str!("../../../../deploy/product/lamp-tasks.service");
    let cloud = include_str!("../../../../deploy/product/lamp-cloud.service");
    let switch = format!("Environment={SWITCH}=on");
    for (unit, text) in [("lamp-tasks", tasks), ("lamp-cloud", cloud)] {
        assert!(text.lines().any(|line| line == switch), "{unit}");
    }
    let serve = tasks
        .lines()
        .find(|line| line.starts_with("ExecStart="))
        .expect("lamp-tasks' ExecStart");
    assert!(serve.contains(" --engines-from-machines "), "{serve}");
    assert!(!cloud.contains("lampos-hermes"));
    // The skills switch (#6128): named by no product unit at all, so it is
    // lamp-update's build default, on on a product lamp.
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    for entry in fs::read_dir(&product).unwrap() {
        let path = entry.unwrap().path();
        if let Ok(text) = fs::read_to_string(&path) {
            assert!(!text.contains(SKILLS_SWITCH), "{}", path.display());
        }
    }
}

/// The skills switch (#6128) is read as [`SWITCH`] is, on by default on
/// every product lamp (the product owner's decision, 11 Nov: a person has
/// no way to turn it on): unset is on on a product lamp, off on any other;
/// set, only `on` is on, so `off` turns a product lamp off.
#[test]
fn the_skills_switch_is_on_by_default_on_every_lamp() {
    const { assert!(SKILLS_PRODUCT_DEFAULT) };
    assert_eq!(SKILLS_SWITCH, "LAMP_HERMES_SKILLS");
    assert!(
        wanted(None, true, SKILLS_PRODUCT_DEFAULT),
        "a product lamp, unset"
    );
    assert!(
        !wanted(None, false, SKILLS_PRODUCT_DEFAULT),
        "another lamp, unset"
    );
    for product in [true, false] {
        for value in ["off", "", "ON", "1", "yes"] {
            assert!(!wanted(Some(value), product, SKILLS_PRODUCT_DEFAULT));
        }
        assert!(wanted(Some("on"), product, SKILLS_PRODUCT_DEFAULT));
    }
}

/// [`ns_text`]: off, exactly [`NS_TEXT`]; on, each of [`SKILLS_EDITS`]
/// found once in [`NS_TEXT`] and made, so the text names
/// [`SKILLS_SOCKET`] and stages, moves back, remounts read-only and
/// removes its staging folder; nothing else of the script changes.
#[test]
fn the_skills_text_is_the_namespace_script_with_only_the_skills_socket_added() {
    assert_eq!(ns_text(false, false), agent_ns::with_ns_path(NS_TEXT));
    assert!(!NS_TEXT.contains(SKILLS_SOCKET));
    assert!(!NS_TEXT.contains("$C"));
    let on = ns_text(true, false);
    for (from, to) in SKILLS_EDITS {
        assert_eq!(NS_TEXT.matches(from).count(), 1, "{from}");
        assert!(on.contains(to), "{to}");
    }
    assert!(on.contains(&format!("C={SKILLS_SOCKET} ")));
    assert!(on.contains("mount --move \"$S/c\" \"$C\" && ro \"$C\""));
    // Only lines added or widened: every line off is in the text on.
    let lines_on: Vec<&str> = on.lines().collect();
    for line in NS_TEXT.lines().filter(|line| {
        !SKILLS_EDITS
            .iter()
            .any(|(from, _)| from.contains(line) || line.contains(from.trim_end()))
    }) {
        assert!(lines_on.contains(&line), "{line}");
    }
    // The skills' socket is moved back before /run is made read-only and
    // before Hermes runs.
    let shown = on.find("mount --move \"$S/c\"").unwrap();
    let ro_run = on
        .find("mount -o remount,bind,ro,nosuid,nodev,noexec /run")
        .unwrap();
    let hidden = on.find("none /run ||").unwrap();
    let staged = on.find("mount --bind \"$C\" \"$S/c\"").unwrap();
    assert!(staged < hidden && hidden < shown && shown < ro_run);
    // The switch is in the text alone: the script reads no flag file, and
    // nothing of the caller's environment but its ids.
    assert_eq!(
        on.matches("LAMP_").count(),
        NS_TEXT.matches("LAMP_").count() + 1,
        "only the comment names the switch"
    );
}

/// The tasks switch (#6306) is lamp-tasks' own name, off unless `on`
/// exactly on any lamp. The product turns it on (#6071, #6305, #6306,
/// #6432): lamp-update's unit and lamp-tasks' each set it `on`, and no
/// product file sets it to anything else.
#[test]
fn the_tasks_switch_is_lamp_tasks_own_and_on_in_the_product() {
    assert_eq!(TASKS_SWITCH, "LAMP_SKILL_TASKS");
    for product in [true, false] {
        assert!(!wanted(None, product, false), "unset");
        for value in ["off", "", "ON", "1", "yes"] {
            assert!(!wanted(Some(value), product, false), "{value:?}");
        }
        assert!(wanted(Some("on"), product, false));
    }
    let switch = format!("Environment={TASKS_SWITCH}=on");
    let setting = format!("{TASKS_SWITCH}=");
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    // One name turns a skill's tasks on: every unit whose part reads it.
    for unit in [
        "lamp-control",
        "lamp-services",
        "lamp-skills",
        "lamp-skills-manage@",
        "lamp-tasks",
        "lamp-update",
        "lamp-voice",
    ] {
        let text = fs::read_to_string(product.join(format!("{unit}.service"))).unwrap();
        let lines = text.lines().filter(|line| *line == switch).count();
        assert_eq!(lines, 1, "{unit}");
    }
    for entry in fs::read_dir(&product).unwrap() {
        let path = entry.unwrap().path();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines().filter(|line| line.contains(&setting)) {
                assert_eq!(line, switch, "{}", path.display());
            }
        }
    }
}

/// A made skill's code of its own (#6307, the last part of #6071) is on in
/// the product (12 Nov, the lead's ask): `lamp-voice` (the making model's
/// brief and the yes), `lamp-skills` (a made skill's `do.task` loads) and
/// its manage helper (a made skill's scripts held to the cap as it is put
/// on) each set it `on` once, and no product file sets it to anything else.
#[test]
fn the_made_scripts_switch_is_on_in_the_product() {
    let name = lamp_voice::made_skill::SCRIPTS_ENVIRONMENT;
    assert_eq!(name, "LAMP_MADE_SCRIPTS");
    let switch = format!("Environment={name}=on");
    let setting = format!("{name}=");
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    // Every unit whose part reads it.
    for unit in ["lamp-skills", "lamp-skills-manage@", "lamp-voice"] {
        let text = fs::read_to_string(product.join(format!("{unit}.service"))).unwrap();
        let lines = text.lines().filter(|line| *line == switch).count();
        assert_eq!(lines, 1, "{unit}");
    }
    for entry in fs::read_dir(&product).unwrap() {
        let path = entry.unwrap().path();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines().filter(|line| line.contains(&setting)) {
                assert_eq!(line, switch, "{}", path.display());
            }
        }
    }
}

/// The walk's few things to try first (#5316) is on in the product: only
/// `lamp-voice` reads it, which sets it `on` once, and no product file sets
/// it to anything else.
#[test]
fn the_first_tries_switch_is_on_in_the_product() {
    let name = lamp_voice::onboarding::FIRST_TRIES_ENVIRONMENT;
    assert_eq!(name, "LAMP_ONBOARDING_FIRST_TRIES");
    let switch = format!("Environment={name}=on");
    let setting = format!("{name}=");
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let text = fs::read_to_string(product.join("lamp-voice.service")).unwrap();
    assert_eq!(text.lines().filter(|line| *line == switch).count(), 1);
    for entry in fs::read_dir(&product).unwrap() {
        let path = entry.unwrap().path();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines().filter(|line| line.contains(&setting)) {
                assert_eq!(line, switch, "{}", path.display());
            }
        }
    }
}

/// A known face giving another name is asked, and only their yes renames
/// it (#5485, 9 of 9 on lamp-52e6, 9 Nov): both rename switches are on in
/// the product. Only `lamp-voice` reads them, which sets each `on` once,
/// and no product file sets either to anything else.
#[test]
fn the_rename_switches_are_on_in_the_product() {
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let voice = fs::read_to_string(product.join("lamp-voice.service")).unwrap();
    assert!(
        voice
            .lines()
            .any(|line| line == "Environment=LAMP_NAMES=checked")
    );
    for (name, want) in [
        (
            lamp_voice::known_faces::RENAME_ASKED_ENVIRONMENT,
            "LAMP_NAMES_RENAME_ASKED",
        ),
        (
            lamp_voice::known_faces::RENAME_YES_ENVIRONMENT,
            "LAMP_NAMES_RENAME_YES",
        ),
    ] {
        assert_eq!(name, want);
        let switch = format!("Environment={name}=on");
        let setting = format!("{name}=");
        assert_eq!(
            voice.lines().filter(|line| *line == switch).count(),
            1,
            "{name}"
        );
        for entry in fs::read_dir(&product).unwrap() {
            let path = entry.unwrap().path();
            if let Ok(text) = fs::read_to_string(&path) {
                for line in text.lines().filter(|line| line.contains(&setting)) {
                    assert_eq!(line, switch, "{}", path.display());
                }
            }
        }
    }
}

/// [`ns_text`] with [`TASKS_SWITCH`] on: each of [`TASKS_EDITS`] found
/// once in [`NS_TEXT`] (and once with [`SKILLS_EDITS`] made) and made, so
/// the text names [`CONTROL_SOCKET`], the file alone, and stages it before
/// `/run` is hidden, moves it back after, remounts it read-only and
/// removes its staging file, all before `/run` is made read-only and
/// Hermes runs. `admin.sock` and the folder alone are never named.
/// Off, the text is unchanged.
#[test]
fn the_tasks_text_is_the_namespace_script_with_only_the_control_socket_added() {
    assert_eq!(ns_text(false, false), agent_ns::with_ns_path(NS_TEXT));
    assert!(!NS_TEXT.contains("lampos-control"));
    assert!(!NS_TEXT.contains("\"$L\""));
    let with_skills = ns_text(true, false);
    let on = ns_text(false, true);
    for (from, to) in TASKS_EDITS {
        assert_eq!(NS_TEXT.matches(from).count(), 1, "{from}");
        assert_eq!(with_skills.matches(from).count(), 1, "{from}");
        assert!(on.contains(to), "{to}");
    }
    assert!(on.contains(&format!(" L={CONTROL_SOCKET} ")));
    assert!(!on.contains("admin.sock"));
    assert!(!on.contains("L=/run/lampos-control "));
    assert_eq!(
        on.matches("lampos-control").count(),
        2,
        "the comment and L="
    );
    // Only lines added or widened: every line off is in the text on.
    let lines_on: Vec<&str> = on.lines().collect();
    for line in NS_TEXT.lines().filter(|line| {
        !TASKS_EDITS
            .iter()
            .any(|(from, _)| from.contains(line) || line.contains(from.trim_end()))
    }) {
        assert!(lines_on.contains(&line), "{line}");
    }
    let staged = on
        .find("if [ -S \"$L\" ]; then touch \"$S/l\" && mount --bind \"$L\" \"$S/l\"")
        .unwrap();
    let hidden = on.find("none /run ||").unwrap();
    let shown = on
        .find("mount --move \"$S/l\" \"$L\" && ro \"$L\"")
        .unwrap();
    let unstaged = on.find("rm -f \"$S/r\" \"$S/l\" \"$S/b\" &&").unwrap();
    let ro_run = on
        .find("mount -o remount,bind,ro,nosuid,nodev,noexec /run")
        .unwrap();
    let hermes = on.find("exec /usr/bin/unshare --user").unwrap();
    assert!(staged < hidden && hidden < shown && shown < unstaged);
    assert!(unstaged < ro_run && ro_run < hermes);
    assert_eq!(
        on.matches("LAMP_").count() - on.matches(NET_ENV).count() - on.matches(HOME_ENV).count(),
        NS_TEXT.matches("LAMP_").count() + 1,
        "only the comment names the switch"
    );
    assert!(!NS_TEXT.contains(NET_ENV) && !NS_TEXT.contains(MODEL_SOCKET));
}

/// With [`TASKS_SWITCH`] on, a run started with [`NET_ENV`] `run` (and
/// only then: every step tests it) also gets: [`MODEL_SOCKET`], the file
/// alone, staged before `/run` is hidden and shown read-only after; `lo`
/// brought up with `ip` from its two places, failing closed without one;
/// and [`RELAY`] `net-relay` from [`RELAY_LISTEN`] to the bridge, started
/// in the background as the caller's own ids in a nested user namespace,
/// its `ready` waited for; all after `/run` is read-only and before Hermes.
#[test]
fn a_net_run_shows_the_bridge_brings_lo_up_and_starts_the_relay() {
    let on = ns_text(false, true);
    for line in [
        " L=/run/lampos-control/control.sock B=/run/lampos-tasks/model.sock P=/var/lib/lampOS/skills-dev S=/tmp/.lampos-ns\n",
        "if [ \"${LAMP_NS_NET-}\" = run ]; then [ -S \"$B\" ] && touch \"$S/b\" && mount --bind \"$B\" \"$S/b\" || fail \"no $B\"; fi\n",
        "if [ \"${LAMP_NS_NET-}\" = run ]; then mkdir -p \"${B%/*}\" && touch \"$B\" && mount --move \"$S/b\" \"$B\" && ro \"$B\" || fail \"$B not shown\"; fi\n",
        "if [ \"${LAMP_NS_NET-}\" = run ] && [ -d \"$P\" ]; then mkdir -m 0700 \"$S/p\" && mount --bind \"$P\" \"$S/p\" || fail \"no $P\"; else P=; fi\n",
        "if [ -n \"$P\" ]; then mkdir -p \"$P\" && mount --move \"$S/p\" \"$P\" && ro \"$P\" || fail \"$P not shown\"; fi\n",
        "  I=; for p in /usr/sbin/ip /sbin/ip; do if [ -x \"$p\" ]; then I=$p; break; fi; done\n",
        "  [ -n \"$I\" ] || fail \"no ip: lo not up, the run has no way out\"\n",
        "  \"$I\" link set lo up || fail \"lo not up\"\n",
        "  [ \"$M\" = ready ] || fail \"no model relay on 127.0.0.1:7741\"\n",
        "unset LAMP_NS_NET\n",
    ] {
        assert_eq!(on.matches(line).count(), 1, "{line}");
    }
    assert!(on.contains(&format!(
        "  M=$(/usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" {RELAY} net-relay \
         --listen {RELAY_LISTEN} --to \"$B\" &)\n"
    )));
    assert!(on.contains(&format!(" B={MODEL_SOCKET} ")));
    assert_eq!(RELAY_LISTEN, "127.0.0.1:7741");
    // Every step of its own is behind the variable, `run` exactly.
    assert_eq!(
        on.matches("if [ \"${LAMP_NS_NET-}\" = run ]; then").count(),
        3
    );
    let staged = on.find("mount --bind \"$B\" \"$S/b\"").unwrap();
    let hidden = on.find("none /run ||").unwrap();
    let shown = on
        .find("mount --move \"$S/b\" \"$B\" && ro \"$B\"")
        .unwrap();
    let ro_run = on
        .find("mount -o remount,bind,ro,nosuid,nodev,noexec /run")
        .unwrap();
    let ids = on.find("U=$LAMP_NS_UID G=$LAMP_NS_GID").unwrap();
    let lo = on.find("link set lo up").unwrap();
    let relay = on.find("net-relay --listen").unwrap();
    let hermes = on.find("exec /usr/bin/unshare --user").unwrap();
    assert!(staged < hidden && hidden < shown && shown < ro_run);
    assert!(ro_run < ids && ids < lo && lo < relay && relay < hermes);
    // The relay's own program is lamp-tasks.service's, under /opt, which
    // the script never hides.
    let unit = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product/lamp-tasks.service"),
    )
    .unwrap();
    assert!(unit.contains(&format!("ExecStart={RELAY} serve ")));
    assert!(!OVERLAID.iter().any(|over| RELAY.starts_with(over)));
    // Its socket is lamp-tasks', beside its others in its runtime folder.
    assert!(unit.contains("RuntimeDirectory=lampos-tasks\nRuntimeDirectoryMode=0700\n"));
    assert_eq!(MODEL_SOCKET, "/run/lampos-tasks/model.sock");
    // Off, none of it.
    for skills in [false, true] {
        let off = ns_text(skills, false);
        assert!(!off.contains(NET_ENV) && !off.contains("net-relay") && !off.contains("lo up"));
    }
}

/// [`wrapper_text`]: off, [`WRAPPER_TEXT`] unchanged; with
/// [`TASKS_SWITCH`] on, `--net` is added to the first `unshare` only when
/// the caller's [`NET_ENV`] is `run` exactly, and the variable passed on
/// then (unset otherwise). Run under `sh` with a stand-in `unshare`.
#[test]
fn the_wrapper_adds_net_only_for_a_net_run_with_the_tasks_switch_on() {
    assert_eq!(
        wrapper_text(false),
        agent_ns::with_hook_note(WRAPPER_TEXT, "hermes")
    );
    for (from, _) in WRAPPER_TASKS_EDITS {
        assert_eq!(WRAPPER_TEXT.matches(from).count(), 1, "{from}");
    }
    assert!(!WRAPPER_TEXT.contains("--net") && !WRAPPER_TEXT.contains(NET_ENV));
    let on = wrapper_text(true);
    assert!(on.ends_with(&format!(
        "if [ \"${{LAMP_NS_NET-}}\" = run ]; then N=--net; else N=; unset LAMP_NS_NET; fi\n\
         exec /usr/bin/unshare --map-root-user $N --mount --pid --kill-child --mount-proc \
         --propagation private {NS} \"$@\"\n"
    )));
    assert_eq!(on.matches("exec ").count(), 1, "one way to run Hermes");
    let folder = tempfile::tempdir().unwrap();
    let printer = folder.path().join("unshare");
    fs::write(
        &printer,
        "#!/bin/sh\necho \"net=${LAMP_NS_NET-unset}\"\necho \"args: $*\"\n",
    )
    .unwrap();
    fs::set_permissions(&printer, fs::Permissions::from_mode(0o755)).unwrap();
    let token = folder.path().join("hermes-token");
    fs::write(&token, "tok_0123456789abcdef0123456789abcdef\n").unwrap();
    let ran = |text: &str, net: Option<&str>| {
        let wrapper = folder.path().join("hermes");
        fs::write(
            &wrapper,
            text.replace(TOKEN, &token.to_string_lossy())
                .replace("/usr/bin/unshare", &printer.to_string_lossy()),
        )
        .unwrap();
        let mut command = std::process::Command::new("/bin/sh");
        command.arg(&wrapper).args(["chat", "-q", "a"]);
        command.env_remove(NET_ENV);
        if let Some(net) = net {
            command.env(NET_ENV, net);
        }
        let out = command.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let plain = format!(
        "args: --map-root-user --mount --pid --kill-child --mount-proc --propagation private {NS} chat -q a\n"
    );
    let netted = format!(
        "net=run\nargs: --map-root-user --net --mount --pid --kill-child --mount-proc \
         --propagation private {NS} chat -q a\n"
    );
    assert_eq!(ran(&on, Some("run")), netted);
    for other in [None, Some(""), Some("RUN"), Some("on"), Some("run ")] {
        assert_eq!(ran(&on, other), format!("net=unset\n{plain}"), "{other:?}");
    }
    // Off: the variable takes nothing away and adds nothing either.
    assert_eq!(ran(WRAPPER_TEXT, Some("run")), format!("net=run\n{plain}"));
    assert_eq!(ran(WRAPPER_TEXT, None), format!("net=unset\n{plain}"));
}

/// #6510 Part 1 step 2: Hermes's wrapper with [`TASKS_SWITCH`] on finds a
/// skill's run by its folder alone ([`RUN_LINES`]): started in
/// [`RUN_FOLDERS`]`/skill-<run-id>`, it reads [`RUNS`]`/<run-id>` once,
/// deletes it, and gives the run the token, the socket, the SDK's
/// `PYTHONPATH` and `--net`; a second start there finds no token and runs
/// nothing; a link at the token's place is refused; any other folder is
/// as before (lamp-tasks' `HermesRunner` still sets [`NET_ENV`] itself).
/// Off, the wrapper has none of it. Run under `sh` with a stand-in
/// `unshare` that prints its environment.
#[test]
fn hermes_finds_a_skill_run_by_its_folder_and_reads_its_token_once() {
    assert!(!WRAPPER_TEXT.contains("skill-") && !WRAPPER_TEXT.contains(RUNS));
    let on = wrapper_text(true);
    assert_eq!(on.matches(RUN_LINES).count(), 1);
    // The run's file goes before the forwarder token is read.
    assert!(on.find(RUN_LINES).unwrap() < on.find("T=$(cat ").unwrap());
    let folder = tempfile::tempdir().unwrap();
    let top = fs::canonicalize(folder.path()).unwrap();
    for dir in [
        "runs",
        "project/lamp",
        "project/skill-run-1",
        "project/skill-Run-2",
    ] {
        fs::create_dir_all(top.join(dir)).unwrap();
    }
    let printer = top.join("unshare");
    fs::write(
        &printer,
        "#!/bin/sh\necho \"args: $*\"\nenv | grep -E '^(LAMP_CONTROL_TOKEN|LAMP_CONTROL_SOCKET|PYTHONPATH|LAMP_NS_NET|HERMES_YOLO_MODE)=' | sort\n",
    )
    .unwrap();
    fs::set_permissions(&printer, fs::Permissions::from_mode(0o755)).unwrap();
    let forwarder = top.join("hermes-token");
    fs::write(&forwarder, "tok_0123456789abcdef0123456789abcdef\n").unwrap();
    let wrapper = top.join("hermes");
    fs::write(
        &wrapper,
        on.replace(TOKEN, &forwarder.to_string_lossy())
            .replace(RUNS, &top.join("runs").to_string_lossy())
            .replace(RUN_FOLDERS, &top.join("project").to_string_lossy())
            .replace("/usr/bin/unshare", &printer.to_string_lossy()),
    )
    .unwrap();
    let run = |cwd: &str, env: &[(&str, &str)]| {
        let out = std::process::Command::new("/bin/sh")
            .arg(&wrapper)
            .args(["chat", "-q", "a"])
            .current_dir(top.join(cwd))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .envs(env.iter().copied())
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let secret = "run-token-0123456789";
    let token = top.join("runs/run-1");
    fs::write(&token, secret).unwrap();
    let (ok, out, err) = run("project/skill-run-1", &[]);
    assert!(ok, "{err}");
    assert!(!token.exists(), "read once and deleted");
    assert_eq!(
        out,
        format!(
            "args: --map-root-user --net --mount --pid --kill-child --mount-proc \
             --propagation private {NS} chat -q a\n\
             HERMES_YOLO_MODE=1\n\
             LAMP_CONTROL_SOCKET={CONTROL_SOCKET}\n\
             LAMP_CONTROL_TOKEN={secret}\n\
             LAMP_NS_NET=run\n\
             PYTHONPATH={SDK_PYTHONPATH}\n"
        )
    );
    assert!(!err.contains(secret));
    // A second start in the same folder: no token, nothing run.
    let (ok, out, err) = run("project/skill-run-1", &[]);
    assert!(!ok && out.is_empty(), "{out}");
    assert!(err.contains("hermes: no token for run run-1"), "{err}");
    // A link at the token's place: never read through, removed, refused.
    let elsewhere = top.join("elsewhere");
    fs::write(&elsewhere, secret).unwrap();
    symlink(&elsewhere, &token).unwrap();
    let (ok, out, _) = run("project/skill-run-1", &[]);
    assert!(!ok && out.is_empty());
    assert!(fs::symlink_metadata(&token).is_err() && elsewhere.exists());
    // Any other folder, a run-like one with an id that is not plain among
    // them: as before, the caller's own LAMP_NS_NET deciding `--net`.
    fs::write(top.join("runs/Run-2"), secret).unwrap();
    for cwd in ["project/lamp", "project", "project/skill-Run-2"] {
        // Hermes's approvals are off only in a skill's run (#6684): one
        // inherited from elsewhere is dropped.
        let (ok, out, err) = run(cwd, &[("HERMES_YOLO_MODE", "1")]);
        assert!(ok, "{cwd}: {err}");
        assert_eq!(
            out,
            format!(
                "args: --map-root-user --mount --pid --kill-child --mount-proc \
                 --propagation private {NS} chat -q a\n"
            ),
            "{cwd}"
        );
        let (ok, out, _) = run(
            cwd,
            &[
                (NET_ENV, "run"),
                ("LAMP_CONTROL_TOKEN", "given"),
                ("HERMES_YOLO_MODE", "1"),
            ],
        );
        assert!(ok && out.contains(" --net ") && out.contains("LAMP_CONTROL_TOKEN=given\n"));
        // Outside a skill's run folder (a spoken task, the owner's own
        // Hermes, any other caller) Hermes keeps its approvals (#6684).
        assert!(!out.contains("HERMES_YOLO_MODE"), "{cwd}: {out}");
    }
    assert!(top.join("runs/Run-2").exists());
    // Off: no run lines; a run's folder is any folder, and an inherited
    // HERMES_YOLO_MODE is dropped all the same (Heron's P3 on #6718).
    let off = wrapper_text(false);
    assert!(!off.contains(RUN_LINES));
    fs::write(
        &wrapper,
        off.replace(TOKEN, &forwarder.to_string_lossy())
            .replace("/usr/bin/unshare", &printer.to_string_lossy()),
    )
    .unwrap();
    fs::write(&token, secret).unwrap();
    for cwd in ["project/skill-run-1", "project/lamp"] {
        let (ok, out, err) = run(cwd, &[("HERMES_YOLO_MODE", "1")]);
        assert!(ok, "{cwd}: {err}");
        assert!(!out.contains("HERMES_YOLO_MODE"), "{cwd}: {out}");
    }
    assert!(token.exists(), "off: no run, its file untouched");
}

/// The first builder's 19:14 ruling on #6643: a voice make's run gets a
/// fresh home of its own. With [`TASKS_SWITCH`] on, the wrapper takes
/// [`HOME_ENV`] only for a make's home ([`MAKE_HOME_GLOB`], never with
/// `..`) and puts `HERMES_HOME` under it; the namespace script then makes
/// it `HOME` and Hermes's lasting folder read-only. Off, neither line.
/// Run under `sh` with a stand-in `unshare`.
#[test]
fn a_makes_own_home_is_taken_only_for_a_makes_folder() {
    let on = wrapper_text(true);
    assert_eq!(on.matches(WRAPPER_HOME_LINE).count(), 1);
    assert!(!wrapper_text(false).contains(HOME_ENV));
    for skills in [false, true] {
        assert_eq!(ns_text(skills, true).matches(NS_HOME_LINE).count(), 1);
        assert!(!ns_text(skills, false).contains(HOME_ENV));
    }
    assert!(WRAPPER_HOME_LINE.contains(MAKE_HOME_GLOB));
    // The home is taken before Hermes is run, as the caller's own ids.
    let ns = ns_text(false, true);
    let home = ns.find(NS_HOME_LINE).unwrap();
    assert!(home > ns.find("U=$LAMP_NS_UID").unwrap());
    assert!(home < ns.find("exec /usr/bin/unshare --user").unwrap());
    let folder = tempfile::tempdir().unwrap();
    let printer = folder.path().join("unshare");
    fs::write(
        &printer,
        "#!/bin/sh\necho \"home=${LAMP_NS_HOME-unset} hermes=$HERMES_HOME\"\n",
    )
    .unwrap();
    fs::set_permissions(&printer, fs::Permissions::from_mode(0o755)).unwrap();
    let token = folder.path().join("hermes-token");
    fs::write(&token, "tok_0123456789abcdef0123456789abcdef\n").unwrap();
    let wrapper = folder.path().join("hermes");
    fs::write(
        &wrapper,
        on.replace(TOKEN, &token.to_string_lossy())
            .replace("/usr/bin/unshare", &printer.to_string_lossy()),
    )
    .unwrap();
    let ran = |home: Option<&str>| {
        let mut command = std::process::Command::new("/bin/sh");
        command.arg(&wrapper).args(["chat", "-q", "a"]);
        command.env_remove(HOME_ENV).env_remove(NET_ENV);
        if let Some(home) = home {
            command.env(HOME_ENV, home);
        }
        let out = command.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let make = "/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-task-1-2-3/home";
    assert_eq!(
        ran(Some(make)),
        format!("home={make} hermes={make}/.hermes\n")
    );
    let lasting = "home=unset hermes=/var/lib/lampos-hermes/.hermes\n";
    for other in [
        None,
        Some(""),
        Some("/var/lib/lampos-hermes"),
        Some(
            "/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-x/../../../../../lampos-hermes/home",
        ),
        Some("/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-x/home/"),
    ] {
        assert_eq!(ran(other), lasting, "{other:?}");
    }
}

/// OpenClaw shares Hermes's namespace script without the tasks edits, and
/// its own wrapper: never `--net`, never the bridge or the relay.
#[test]
fn openclaw_never_gets_a_net_namespace() {
    for skills in [false, true] {
        let text = crate::openclaw_install::ns_text(Shown {
            skills,
            tasks: false,
            one_path: false,
        });
        assert!(!text.contains(NET_ENV) && !text.contains("net-relay"));
        assert!(!text.contains(MODEL_SOCKET));
    }
    assert!(!crate::openclaw_install::WRAPPER_TEXT.contains("--net"));
    assert!(!crate::openclaw_install::WRAPPER_TEXT.contains(NET_ENV));
}

/// Both switches on: every edit of both made, both sockets staged and
/// shown read-only, and still never `admin.sock`.
#[test]
fn both_switches_on_make_both_edits() {
    let both = ns_text(true, true);
    // Every tasks edit whole; every skills edit whole once the tasks
    // edits are made in it too (two widen the same lines).
    for (_, to) in TASKS_EDITS {
        assert!(both.contains(to), "{to}");
    }
    for (_, to) in SKILLS_EDITS {
        let mut to = to.to_owned();
        for (from, tasks) in TASKS_EDITS {
            to = to.replacen(from, tasks, 1);
        }
        assert!(both.contains(&to), "{to}");
    }
    for (from, _) in TASKS_EDITS {
        assert_eq!(ns_text(true, false).matches(from).count(), 1, "{from}");
    }
    assert!(both.contains("mount --move \"$S/c\" \"$C\" && ro \"$C\""));
    assert!(both.contains("mount --move \"$S/l\" \"$L\" && ro \"$L\""));
    assert!(both.contains(
        " C=/run/lampos-skills L=/run/lampos-control/control.sock B=/run/lampos-tasks/model.sock P=/var/lib/lampOS/skills-dev S="
    ));
    assert!(!both.contains("admin.sock"));
    assert_eq!(
        both.matches("LAMP_").count()
            - both.matches(NET_ENV).count()
            - both.matches(HOME_ENV).count(),
        NS_TEXT.matches("LAMP_").count() + 2
    );
}

/// Turning the tasks switch on with Hermes there (written with it off):
/// a round finds the script not as this build puts it and asks with
/// `tasks`; the one-shot runs no installer stage and writes the script
/// with the control socket. Off again writes it back without.
#[test]
fn turning_the_tasks_switch_on_or_off_writes_the_namespace_script_again() {
    let lamp = Lamp::new();
    lamp.harness_there();
    lamp.hermes_there();
    assert!(present(lamp.root(), OFF) && !present(lamp.root(), TASKS_ON));
    let mut systemd = Fake::default();
    assert_eq!(round(&lamp, &mut systemd, 5).unwrap().install, "installed");
    let with_tasks = |lamp: &Lamp, systemd: &mut Fake, at: u64| {
        after_round(
            lamp.root(),
            &lamp.records(),
            TASKS_ON,
            systemd,
            at,
            Ok(crate::hermes_schedule::Window::Reached),
            &mut quiet(),
        )
    };
    assert_eq!(
        with_tasks(&lamp, &mut systemd, 10).unwrap().install,
        "installing"
    );
    let text = fs::read_to_string(lamp.records().join(ASK)).unwrap();
    assert!(
        text.contains("\"tasks\":true") && !text.contains("skills"),
        "{text}"
    );
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 20).unwrap().install, "installed");
    assert!(runs.stages().is_empty(), "no installer stage");
    assert_eq!(
        fs::read_to_string(lamp.at(NS)).unwrap(),
        ns_text(false, true)
    );
    assert!(present(lamp.root(), TASKS_ON) && !present(lamp.root(), OFF));
    assert_eq!(with_tasks(&lamp, &mut systemd, 30), None, "nothing to do");

    // Off again.
    assert_eq!(
        round(&lamp, &mut systemd, 40).unwrap().install,
        "installing"
    );
    let text = fs::read_to_string(lamp.records().join(ASK)).unwrap();
    assert!(!text.contains("tasks"), "{text}");
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 50).unwrap().install, "installed");
    assert_eq!(
        fs::read_to_string(lamp.at(NS)).unwrap(),
        ns_text(false, false)
    );
}

/// Turning the skills switch on with Hermes there (written with it off):
/// a round finds Hermes not there as this build puts it and asks the
/// one-shot with `skills`; the one-shot runs no installer stage, writes
/// the namespace script with the skills' socket, and Hermes is there.
/// Turning it off again writes the script back without it.
#[test]
fn turning_the_skills_switch_on_or_off_writes_the_namespace_script_again() {
    let lamp = Lamp::new();
    lamp.harness_there();
    lamp.hermes_there();
    assert!(present(lamp.root(), OFF));
    assert!(!present(lamp.root(), SKILLS_ON));
    let mut systemd = Fake::default();
    assert_eq!(round(&lamp, &mut systemd, 5).unwrap().install, "installed");
    let said = round_with_skills(&lamp, &mut systemd, 10).unwrap();
    assert_eq!(said.install, "installing");
    assert_eq!(
        lamp.asked(),
        Some(Ask {
            version: 1,
            remove: false,
            skills: true,
            tasks: false,
            one_path: false,
        })
    );
    let text = fs::read_to_string(lamp.records().join(ASK)).unwrap();
    assert!(text.contains("\"skills\":true"), "{text}");
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 20).unwrap().install, "installed");
    assert!(runs.stages().is_empty(), "no installer stage");
    assert_eq!(
        fs::read_to_string(lamp.at(NS)).unwrap(),
        ns_text(true, false)
    );
    assert_eq!(
        fs::read_to_string(lamp.at(WRAPPER)).unwrap(),
        wrapper_text(false)
    );
    assert_eq!(mode(&lamp.at(NS)), 0o755);
    assert!(present(lamp.root(), SKILLS_ON) && !present(lamp.root(), OFF));
    assert_eq!(
        round_with_skills(&lamp, &mut systemd, 30),
        None,
        "nothing to do"
    );

    // Off again.
    assert_eq!(
        round(&lamp, &mut systemd, 40).unwrap().install,
        "installing"
    );
    assert_eq!(lamp.asked(), Some(Ask::default_with(1)));
    let text = fs::read_to_string(lamp.records().join(ASK)).unwrap();
    assert!(!text.contains("skills"), "{text}");
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 50).unwrap().install, "installed");
    assert!(runs.stages().is_empty());
    assert_eq!(
        fs::read_to_string(lamp.at(NS)).unwrap(),
        ns_text(false, false)
    );
    assert!(present(lamp.root(), OFF));
}

/// An ask an older build wrote, with no `skills`, is read as off: the
/// namespace script shows nothing under `/run`.
#[test]
fn an_ask_without_skills_is_off() {
    let ask: Ask = serde_json::from_str("{\"version\":1}").unwrap();
    assert_eq!(ask, Ask::default_with(1));
    let lamp = Lamp::new();
    lamp.hermes_there();
    fs::write(lamp.at(NS), ns_text(true, false)).unwrap();
    lamp.ask();
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 5).unwrap().install, "installed");
    assert_eq!(
        fs::read_to_string(lamp.at(NS)).unwrap(),
        ns_text(false, false)
    );
}

/// Under 1 GiB of memory available (`MemAvailable`; lamp-c0a4 had about
/// 2.1 GB at rest with the voice running), or it not read: `no_memory`
/// before the disk is looked at, and nothing written under Hermes's place,
/// no stage run. Exactly enough is enough.
#[test]
fn too_little_memory_is_no_memory_and_nothing_is_made() {
    let short = format!("MemAvailable: {} kB\n", NEEDS_AVAILABLE_KB - 1);
    // Each case's /proc/meminfo; none is no file at all.
    let cases = [
        ("under the floor", Some(short.as_str())),
        ("not a number", Some("MemAvailable: lots\n")),
        ("no unit", Some("MemAvailable: 9999999\n")),
        (
            "no MemAvailable",
            Some("MemTotal: 8045532 kB\nMemFree: 9999999 kB\n"),
        ),
        ("no file", None),
    ];
    for (case, meminfo) in cases {
        let lamp = Lamp::new();
        lamp.ask();
        match meminfo {
            Some(text) => lamp.meminfo(text),
            None => fs::remove_file(lamp.at(MEMINFO)).unwrap(),
        }
        let runs = Runs::new(&lamp);
        let said = install(&lamp, &runs, 5).unwrap();
        assert_eq!(said.install, "failed", "{case}");
        assert_eq!(said.error.as_deref(), Some("no_memory"), "{case}");
        assert!(lamp.said().unwrap().check(), "{case}");
        assert!(!runs.ran("df") && !runs.ran("install -d"), "{case}");
        assert!(runs.stages().is_empty(), "{case}");
        assert!(!lamp.at(PLACE).exists(), "{case}");
        assert!(lamp.entry_points().is_empty(), "{case}");
    }
    let lamp = Lamp::new();
    lamp.ask();
    lamp.memory(NEEDS_AVAILABLE_KB);
    let runs = Runs::new(&lamp);
    assert_eq!(install(&lamp, &runs, 5).unwrap().install, "installed");
    assert!(runs.ran("df -Pk /var/lib"));
}

/// Outside its window (#5978's schedule) a round starts nothing and writes
/// no ask, and takes back an install ask an earlier round left, so a
/// one-shot started for Harness alone does not run Hermes's install now;
/// an ask to take the entry points out is kept. Hermes already there is
/// still recorded `installed`, whatever the window.
#[test]
fn a_closed_window_starts_nothing_and_takes_back_an_earlier_ask() {
    let lamp = Lamp::new();
    lamp.harness_there();
    let mut systemd = Fake::default();
    assert_eq!(
        closed_round(&lamp, &mut systemd, 5, "not a quiet stretch"),
        None
    );
    assert!(systemd.asked.is_empty());
    assert_eq!(lamp.asked(), None);
    lamp.ask();
    assert_eq!(
        closed_round(&lamp, &mut systemd, 6, "the first round"),
        None
    );
    assert_eq!(lamp.asked(), None, "an earlier install ask is taken back");
    fs::write(lamp.records().join(ASK), "{\"version\":1,\"remove\":true}").unwrap();
    assert_eq!(closed_round(&lamp, &mut systemd, 7, "night"), None);
    assert!(lamp.asked().unwrap().remove, "an ask to remove is kept");
    fs::remove_file(lamp.records().join(ASK)).unwrap();
    // Open: the install starts.
    assert_eq!(round(&lamp, &mut systemd, 8).unwrap().install, "installing");
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
    let lamp = Lamp::new();
    lamp.hermes_there();
    let said = closed_round(&lamp, &mut Fake::default(), 9, "day").unwrap();
    assert_eq!(said.install, "installed");
}

/// An install the watch stopped (`privacy_lost`, `interrupted`) is
/// recorded so, its ask taken away, and tried again at the next round
/// whose window is open, not a day later; a closed one waits.
#[test]
fn a_stopped_install_is_tried_again_at_the_next_open_window() {
    for error in ["privacy_lost", "interrupted"] {
        let lamp = Lamp::new();
        lamp.harness_there();
        lamp.ask();
        let said = stopped(&lamp.records(), error, 100);
        assert_eq!(
            (said.install.as_str(), said.error.as_deref()),
            ("failed", Some(error))
        );
        assert!(said.check());
        assert_eq!(lamp.asked(), None);
        let mut systemd = Fake::default();
        assert_eq!(closed_round(&lamp, &mut systemd, 200, "day"), None);
        assert!(systemd.asked.is_empty());
        assert_eq!(
            round(&lamp, &mut systemd, 300).unwrap().install,
            "installing"
        );
    }
}

/// The watch's acts on Harness's one-shot, only while systemd says it
/// runs: frozen and thawed with `systemctl freeze`/`thaw`; stopped (thawed
/// first), recorded `failed` with the act's error and its ask taken away.
/// A freeze or thaw refused stops it (`interrupted`), so it is never left
/// frozen. Not running, nothing is done.
#[test]
fn the_watch_freezes_thaws_and_stops_only_a_running_one_shot() {
    use crate::hermes_schedule::Act;
    let lamp = Lamp::new();
    let records = lamp.records();
    let mut systemd = Fake {
        active: Some("activating"),
        ..Fake::default()
    };
    let mut lines = Vec::new();
    let mut say = |line: &str| lines.push(line.to_owned());
    assert_eq!(
        carry_out(&Act::Nothing, &records, &mut systemd, 1, &mut say),
        Carried::Done
    );
    assert!(systemd.asked.is_empty(), "nothing asked of systemd");
    let freeze = Act::Freeze("the voice is hearing someone".into());
    assert_eq!(
        carry_out(&freeze, &records, &mut systemd, 1, &mut say),
        Carried::Done
    );
    assert_eq!(
        carry_out(&Act::Thaw, &records, &mut systemd, 2, &mut say),
        Carried::Done
    );
    assert_eq!(systemd.asked, ["state", "freeze", "state", "thaw"]);
    assert_eq!(lamp.said(), None);
    lamp.ask();
    let stop = Act::Stop {
        error: "privacy_lost",
        why: "the lease is gone".into(),
    };
    systemd.asked.clear();
    assert_eq!(
        carry_out(&stop, &records, &mut systemd, 3, &mut say),
        Carried::Stopped
    );
    assert_eq!(systemd.asked, ["state", "thaw", "stop"]);
    assert_eq!(lamp.said().unwrap().error.as_deref(), Some("privacy_lost"));
    assert_eq!(lamp.asked(), None);
    // A freeze refused: stopped, interrupted.
    let mut refused = Fake {
        active: Some("activating"),
        lever: Some(Err("no".into())),
        ..Fake::default()
    };
    assert_eq!(
        carry_out(&freeze, &records, &mut refused, 4, &mut say),
        Carried::Stopped
    );
    assert_eq!(refused.asked, ["state", "freeze", "thaw", "stop"]);
    assert_eq!(lamp.said().unwrap().error.as_deref(), Some("interrupted"));
    // Not running: nothing.
    for active in ["inactive", "failed", "active"] {
        let mut ended = Fake {
            active: Some(active),
            ..Fake::default()
        };
        assert_eq!(
            carry_out(&stop, &records, &mut ended, 5, &mut say),
            Carried::NotRunning
        );
        assert_eq!(ended.asked, ["state"]);
    }
    assert_eq!(lamp.said().unwrap().at_unix_ms, 4);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("frozen while the voice is not quiet")),
        "{lines:?}"
    );
}

/// A round in the window the app's tap opens (#6240).
fn asked_round(lamp: &Lamp, systemd: &mut Fake, at: u64) -> Option<HermesInstall> {
    after_round(
        lamp.root(),
        &lamp.records(),
        Shown::default(),
        systemd,
        at,
        Ok(crate::hermes_schedule::Window::Asked),
        &mut quiet(),
    )
}

/// The app's tap (#6240) is a deliberate try again: a recent failure that
/// waits a day for a round is started at once when asked; an install under
/// way is still left alone until it is taken as stuck.
#[test]
fn an_ask_from_the_app_tries_a_recent_failure_again_but_not_an_install_under_way() {
    let lamp = Lamp::new();
    lamp.harness_there();
    lamp.ask();
    let runs = Runs {
        failing: Some("venv"),
        stage: Err("bash: exit status 1".into()),
        ..Runs::new(&lamp)
    };
    assert_eq!(install(&lamp, &runs, 100).unwrap().install, "failed");
    let mut systemd = Fake::default();
    assert_eq!(round(&lamp, &mut systemd, 100 + HOUR_MS), None);
    assert!(systemd.asked.is_empty());
    let asked = asked_round(&lamp, &mut systemd, 100 + HOUR_MS).unwrap();
    assert_eq!(asked.install, "installing");
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
    // Under way: a tap starts nothing more.
    let mut again = Fake::default();
    assert_eq!(asked_round(&lamp, &mut again, 100 + 2 * HOUR_MS), None);
    assert!(again.asked.is_empty());
}

/// (#6530) The shared hook stand-in is written with the namespace script
/// and kept while another engine's namespace script (OpenClaw's) is there:
/// Hermes's entry points taken out leave it for OpenClaw.
#[test]
fn the_hook_stand_in_stays_while_another_engines_namespace_uses_it() {
    let lamp = Lamp::new();
    lamp.hermes_there();
    assert!(present(lamp.root(), OFF));
    fs::write(lamp.at(crate::openclaw_install::NS), "").unwrap();
    remove_entry_points(lamp.root(), &mut quiet());
    assert!(lamp.entry_points().is_empty());
    assert!(
        agent_ns::hook_shim_there(lamp.root()),
        "OpenClaw still uses it"
    );
    fs::remove_file(lamp.at(crate::openclaw_install::NS)).unwrap();
    remove_entry_points(lamp.root(), &mut quiet());
    assert!(!lamp.at(agent_ns::HOOK_SHIM).exists());
    assert!(!lamp.at(agent_ns::BIN).exists());
    // Hermes as this build puts it needs the stand-in: one missing is
    // Hermes missing, written again.
    lamp.hermes_there();
    fs::remove_file(lamp.at(agent_ns::HOOK_SHIM)).unwrap();
    assert!(!present(lamp.root(), OFF));
}

/// (#6530) Every namespace script, for every switch, puts the stand-in's
/// folder first on the `PATH` once, right before the line that runs
/// Hermes, after anything the switches add; and the wrapper says once,
/// outside the namespace, that Harness's hooks aren't there yet.
#[test]
fn every_namespace_script_finds_the_hook_stand_in_first_on_its_path() {
    let path_line =
        "PATH=/usr/local/lib/lampos/ns-bin:${PATH:-/usr/local/bin:/usr/bin:/bin}\nexport PATH\n";
    for skills in [false, true] {
        for tasks in [false, true] {
            let text = ns_text(skills, tasks);
            assert_eq!(text.matches(path_line).count(), 1, "{skills} {tasks}");
            assert!(
                text.ends_with(&format!(
                    "{path_line}exec /usr/bin/unshare --user --map-user=\"$U\" --map-group=\"$G\" {HERMES} \"$@\"\n"
                )),
                "{skills} {tasks}"
            );
            let wrapper = wrapper_text(tasks);
            let note = agent_ns::hook_note_line("hermes");
            assert_eq!(wrapper.matches(&note).count(), 1);
            assert!(wrapper.find(&note) < wrapper.find("LAMP_NS_UID="));
            assert!(wrapper[..512].contains("Written by lamp-update."));
        }
    }
    assert!(agent_ns::hook_note_line("hermes").starts_with(
        "logger -t hermes \"Harness hooks aren't available inside the lamp's agent namespace yet (openharness#4194)\" 2>/dev/null || :"
    ));
}

/// (#6530) The wrapper's journal line never fails a start: with a
/// `logger` that fails, Hermes's namespace is still entered.
#[test]
fn a_failing_logger_never_fails_the_wrapper() {
    let folder = tempfile::tempdir().unwrap();
    let bin = folder.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let logged = folder.path().join("logged");
    for (name, body) in [
        (
            "logger",
            format!("echo \"$*\" > {}\nexit 1\n", logged.display()),
        ),
        ("unshare", "echo ran\n".to_owned()),
    ] {
        let place = bin.join(name);
        fs::write(&place, format!("#!/bin/sh\n{body}")).unwrap();
        fs::set_permissions(&place, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let token = folder.path().join("hermes-token");
    fs::write(&token, "tok\n").unwrap();
    let wrapper = folder.path().join("hermes");
    fs::write(
        &wrapper,
        wrapper_text(false)
            .replace(TOKEN, &token.to_string_lossy())
            .replace("/usr/bin/unshare", &bin.join("unshare").to_string_lossy()),
    )
    .unwrap();
    let out = std::process::Command::new("/bin/sh")
        .arg(&wrapper)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ran\n");
    assert_eq!(
        fs::read_to_string(&logged).unwrap(),
        format!("-t hermes {}\n", agent_ns::HOOK_NOTE)
    );
}

/// #6506: lamp-tasks offers an engine on the lamp only when its program
/// in `/usr/local/bin` carries this line in its first 512 bytes
/// (`lamp_tasks::runtime::LAMP_WRAPPER_MARK`). Hermes's and OpenClaw's
/// wrappers, with the tasks switch on or off, all do.
#[test]
fn every_engine_wrapper_carries_the_mark_lamp_tasks_looks_for() {
    const MARK: &str = "Written by lamp-update.";
    for text in [
        wrapper_text(false),
        wrapper_text(true),
        crate::openclaw_install::WRAPPER_TEXT.to_owned(),
    ] {
        let head = &text.as_bytes()[..text.len().min(512)];
        assert!(
            head.windows(MARK.len()).any(|w| w == MARK.as_bytes()),
            "{}",
            text.lines().nth(1).unwrap_or_default()
        );
    }
}

/// #6685: with the one run path ([`Shown::one_path`]), Hermes's wrapper is
/// the one every agent has, with Hermes's data, and runs the one agent
/// namespace ([`agent_ns::wrapper::NS`]); Hermes's own namespace script
/// is taken out; the ask carries it. Off, every text is as before.
#[test]
fn with_the_one_path_hermes_runs_through_the_one_agent_namespace() {
    let one = Shown {
        skills: true,
        tasks: true,
        one_path: true,
    };
    for off in [OFF, SKILLS_ON, TASKS_ON] {
        assert_eq!(wrapper_for(off), wrapper_text(off.tasks));
        assert_eq!(ns_for(off), (NS, ns_text(off.skills, off.tasks)));
    }
    assert_eq!(
        wrapper_for(one),
        agent_ns::wrapper::one_path_text(&agent()).unwrap()
    );
    assert_eq!(
        ns_for(one),
        (agent_ns::wrapper::NS, agent_ns::wrapper::ns_text(one))
    );
    let lamp = Lamp::new();
    lamp.hermes_there();
    assert!(present(lamp.root(), OFF));
    assert!(
        !present(lamp.root(), one),
        "the one path's texts are not there"
    );
    write_entry_points(lamp.root(), one).unwrap();
    assert_eq!(
        fs::read_to_string(lamp.at(WRAPPER)).unwrap(),
        wrapper_for(one)
    );
    assert_eq!(
        fs::read_to_string(lamp.at(agent_ns::wrapper::NS)).unwrap(),
        agent_ns::wrapper::ns_text(one)
    );
    assert!(!lamp.at(NS).exists(), "nothing runs hermes-ns any more");
    assert!(agent_ns::hook_shim_there(lamp.root()));
    assert!(present(lamp.root(), one));
    assert!(!present(lamp.root(), OFF));
    // Off again: Hermes's own wrapper and script, as before.
    write_entry_points(lamp.root(), OFF).unwrap();
    assert!(present(lamp.root(), OFF));
    assert_eq!(
        fs::read_to_string(lamp.at(WRAPPER)).unwrap(),
        wrapper_text(false)
    );
    // The ask carries the switch; off, its text is as before.
    ask(&lamp.records(), false, one).unwrap();
    assert_eq!(lamp.asked().unwrap().shown(), one);
    ask(&lamp.records(), false, SKILLS_ON).unwrap();
    assert_eq!(
        fs::read_to_string(lamp.records().join(ASK)).unwrap(),
        "{\"version\":1,\"skills\":true}"
    );
}

/// #6684: Hermes's launcher replaces the `PYTHONPATH` it is started with,
/// so [`RUN_ENV`] puts the SDK back in its terminal's login snapshot, for a
/// skill run only: with `LAMP_CONTROL_SOCKET` set, the SDK first and what
/// was there kept, never twice; without it, `PYTHONPATH` as it was.
#[test]
fn the_run_env_puts_the_sdk_on_a_skill_runs_pythonpath_only() {
    assert!(
        RUN_ENV.starts_with("/usr/local/lib/lampos/"),
        "beside the namespace script, root's"
    );
    assert_eq!(
        RUN_ENV_TEXT.matches(SDK_PYTHONPATH).count(),
        3,
        "the one SDK path the wrapper gives a run"
    );
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("hermes-run-env.sh");
    fs::write(&file, RUN_ENV_TEXT).unwrap();
    let sourced = |socket: Option<&str>, path: Option<&str>| {
        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", ". \"$1\"; . \"$1\"; printf '%s|%s' \"${PYTHONPATH-unset}\" \"$(env | grep -c '^PYTHONPATH=')\"", "sh"])
            .arg(&file)
            .env_clear()
            .env("PATH", "/usr/bin:/bin");
        if let Some(socket) = socket {
            command.env("LAMP_CONTROL_SOCKET", socket);
        }
        if let Some(path) = path {
            command.env("PYTHONPATH", path);
        }
        let out = command.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    let socket = Some(CONTROL_SOCKET);
    assert_eq!(
        sourced(socket, None),
        format!("{SDK_PYTHONPATH}|1"),
        "exported"
    );
    assert_eq!(
        sourced(socket, Some("/h/repo:/h/site")),
        format!("{SDK_PYTHONPATH}:/h/repo:/h/site|1"),
        "first, and Hermes's own kept"
    );
    assert_eq!(
        sourced(socket, Some(SDK_PYTHONPATH)),
        format!("{SDK_PYTHONPATH}|1")
    );
    assert_eq!(sourced(None, None), "unset|0", "no run, nothing set");
    assert_eq!(
        sourced(Some(""), Some("/a")),
        "/a|1",
        "an empty socket is no run"
    );
    assert_eq!(sourced(None, Some("/a:/b")), "/a:/b|1");
}

/// The install writes [`RUN_ENV`] root's 0644, never run, and its text is
/// part of Hermes being there: changed or missing, it is written again.
#[test]
fn the_run_env_is_written_with_the_entry_points_and_counted_in_present() {
    let lamp = Lamp::new();
    lamp.hermes_there();
    let shown = OFF;
    assert!(present(lamp.root(), shown));
    fs::write(
        lamp.at(RUN_ENV),
        "PYTHONPATH=/elsewhere; export PYTHONPATH\n",
    )
    .unwrap();
    assert!(
        !present(lamp.root(), shown),
        "another text is Hermes missing"
    );
    fs::remove_file(lamp.at(RUN_ENV)).unwrap();
    assert!(!present(lamp.root(), shown), "so is none");
    write_entry_points(lamp.root(), shown).unwrap();
    assert!(holds(lamp.root(), RUN_ENV, RUN_ENV_TEXT));
    let mode = fs::metadata(lamp.at(RUN_ENV)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644);
    assert!(present(lamp.root(), shown));
}

/// A make's lines on their own, after [`NS`]'s `fail` and `ro`, with `H`
/// and `W` (and the staging folder) moved to `base`'s, and `after` once
/// the lines have run.
fn make_lines(base: &Path, after: &str) -> String {
    let ns = ns_text(false, true);
    let helpers: String = ns
        .lines()
        .filter(|line| line.starts_with("fail() ") || line.starts_with("ro() "))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(helpers.lines().count(), 2);
    format!(
        "#!/bin/sh\n{helpers}H={b}/h W={b}/w\n{}{after}",
        NS_HOME_LINE.replace("/tmp/.lampos-mk", &format!("{}/x", base.display())),
        b = base.display()
    )
}

/// Hermes's lasting home and a lamp-tasks work folder, with a make's folder
/// in it, under `base`.
fn a_lamp_with_a_make(base: &Path) -> PathBuf {
    for (path, text) in [
        ("h/.hermes/memories/MEMORY.md", "THE OWNER'S MEMORY"),
        ("h/.hermes/state.db", "STATE"),
        ("h/.hermes/hermes-agent/run.sh", "echo ran"),
        ("h/.hermes/tools/python-3/bin/python3", "the interpreter"),
        ("h/.hermes/installs/k/facts.json", "the dependency record"),
        ("h/.local/bin/README", "the program"),
        ("w/top.md", "W"),
        ("w/lamp/AGENTS.md", "HARNESS PROJECT"),
        ("w/lamp/make-task-1/rules.md", "R"),
        ("w/lamp/make-task-1/home/.hermes/config.yaml", "C"),
    ] {
        let path = base.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    base.join("w/lamp/make-task-1")
}

/// #6643's HOLDs (the Reviewer-lead 19:58, SubDarren 20:0x, 12 Nov): for
/// a make, which keeps the network, the script hides Hermes's lasting
/// home under an empty tmpfs with only its checkout, `tools` (the
/// Python Hermes runs on) and `installs` (its dependency record, which a
/// make's home links to) shown again, read-only; leaves Hermes's folder and the work folder read-only; hides
/// the rest of `W/lamp` under another tmpfs, the make's own folder alone
/// moved back, read-write; and starts Hermes there with the make's home.
/// Each step failing ends it before Hermes runs; a home outside `W/lamp`
/// is refused; no make, nothing of this runs.
#[test]
fn a_makes_namespace_hides_hermess_lasting_home_and_the_rest_of_the_work_folder() {
    let folder = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(folder.path()).unwrap();
    let make = a_lamp_with_a_make(&base);
    let stubs = base.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let stub = "#!/bin/sh\necho \"${0##*/} $*\" >> \"$LOG\"\n\
                if [ -n \"$FAIL_ON\" ]; then case \"${0##*/} $*\" in *\"$FAIL_ON\"*) exit 1;; esac; fi\n\
                case ${0##*/} in findmnt) echo rw,nosuid,nodev,relatime;; esac\n";
    for name in ["mount", "findmnt", "mkdir", "rmdir"] {
        let path = stubs.join(name);
        fs::write(&path, stub).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let script = base.join("make-lines");
    fs::write(
        &script,
        make_lines(&base, "echo \"HOME=$HOME PWD=$(pwd -P)\"\n"),
    )
    .unwrap();
    let log = base.join("log");
    let run = |home: Option<&Path>, fail_on: &str| {
        let _ = fs::remove_file(&log);
        let mut command = std::process::Command::new("/bin/sh");
        command
            .arg(&script)
            .current_dir(&make)
            .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
            .env("LOG", &log)
            .env("FAIL_ON", fail_on)
            .env("HOME", "/lasting");
        match home {
            Some(home) => command.env(HOME_ENV, home),
            None => command.env_remove(HOME_ENV),
        };
        let out = command.output().unwrap();
        let lines: Vec<String> = fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect();
        (out, lines)
    };
    let (h, w, x) = (base.join("h"), base.join("w"), base.join("x"));
    let (h, w, x, f) = (h.display(), w.display(), x.display(), make.display());
    let home = make.join("home");
    let (out, lines) = run(Some(&home), "");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("HOME={} PWD={f}\n", home.display())
    );
    let ro = |path: String| {
        [
            format!("findmnt -no VFS-OPTIONS -M {path}"),
            format!("mount -o remount,bind,ro,nosuid,nodev,relatime {path}"),
        ]
    };
    let mut wanted = vec![
        format!("mkdir -m 0700 {x} {x}/f {x}/c {x}/t {x}/i"),
        format!("mount --bind {f} {x}/f"),
        format!("mount --bind {h}/.hermes/hermes-agent {x}/c"),
        format!("mount --bind {h}/.hermes/tools {x}/t"),
        format!("mount --bind {h}/.hermes/installs {x}/i"),
        format!("mount -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none {h}/.hermes"),
        format!("mkdir {h}/.hermes/hermes-agent"),
        format!("mount --move {x}/c {h}/.hermes/hermes-agent"),
    ];
    wanted.extend(ro(format!("{h}/.hermes/hermes-agent")));
    wanted.push(format!("mkdir {h}/.hermes/tools"));
    wanted.push(format!("mount --move {x}/t {h}/.hermes/tools"));
    wanted.extend(ro(format!("{h}/.hermes/tools")));
    wanted.push(format!("mkdir {h}/.hermes/installs"));
    wanted.push(format!("mount --move {x}/i {h}/.hermes/installs"));
    wanted.extend(ro(format!("{h}/.hermes/installs")));
    wanted.extend(ro(format!("{h}/.hermes")));
    wanted.extend(ro(h.to_string()));
    wanted.extend(ro(w.to_string()));
    wanted.push(format!(
        "mount -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none {w}/lamp"
    ));
    wanted.push(format!("mkdir {f}"));
    wanted.push(format!("mount --move {x}/f {f}"));
    wanted.extend(ro(format!("{w}/lamp")));
    wanted.push(format!("rmdir {x}/f {x}/c {x}/t {x}/i {x}"));
    assert_eq!(lines, wanted);
    for step in [
        "mkdir -m 0700".to_owned(),
        format!("--bind {f} "),
        format!("--bind {h}/.hermes/hermes-agent "),
        format!("--bind {h}/.hermes/tools "),
        format!("--bind {h}/.hermes/installs "),
        format!("none {h}/.hermes"),
        format!("mkdir {h}/.hermes/hermes-agent"),
        format!("--move {x}/c "),
        format!("relatime {h}/.hermes/hermes-agent"),
        format!("mkdir {h}/.hermes/tools"),
        format!("--move {x}/t "),
        format!("relatime {h}/.hermes/tools"),
        format!("mkdir {h}/.hermes/installs"),
        format!("--move {x}/i "),
        format!("relatime {h}/.hermes/installs"),
        format!("relatime {h}/.hermes"),
        format!("relatime {h}"),
        format!("relatime {w}"),
        format!("none {w}/lamp"),
        format!("mkdir {f}"),
        format!("--move {x}/f "),
        format!("relatime {w}/lamp"),
        "rmdir".to_owned(),
    ] {
        let (out, _) = run(Some(&home), &step);
        assert!(!out.status.success(), "{step}");
        assert!(out.stdout.is_empty(), "Hermes never runs: {step}");
    }
    // A home outside the work folder's `lamp`, or missing, is refused
    // before anything is mounted.
    let other = base.join("other/make-task-1/home");
    fs::create_dir_all(&other).unwrap();
    for home in [other, make.join("gone/home")] {
        let (out, lines) = run(Some(&home), "");
        assert!(!out.status.success(), "{home:?}");
        assert!(lines.is_empty(), "{lines:?}");
    }
    // No make: none of it.
    let (out, lines) = run(None, "");
    assert!(out.status.success());
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("HOME=/lasting PWD={f}\n")
    );
}

/// The same lines run for real in a user and mount namespace of their own
/// (Linux, where the kernel allows one; skipped otherwise): from inside a
/// make's namespace, a `cat` of Hermes's lasting memories finds nothing,
/// its lasting home shows only the checkout, `tools` and `installs`, read-only, `W/lamp` shows
/// only the make's folder, and nothing but the make's folder can be
/// written.
#[cfg(target_os = "linux")]
#[test]
fn inside_a_makes_namespace_hermess_lasting_memory_is_not_there() {
    let folder = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(folder.path()).unwrap();
    let probe = base.join("probe");
    fs::create_dir(&probe).unwrap();
    let allowed = std::process::Command::new("unshare")
        .args(["--map-root-user", "--mount", "--propagation", "private"])
        .args(["mount", "-t", "tmpfs", "none"])
        .arg(&probe)
        .output()
        .is_ok_and(|out| out.status.success());
    if !allowed {
        eprintln!("no user and mount namespace here: skipped");
        return;
    }
    let make = a_lamp_with_a_make(&base);
    let checks = "\
        cat \"$H/.hermes/memories/MEMORY.md\" 2>/dev/null && echo SEEN-MEMORY\n\
        cat \"$H/.hermes/state.db\" 2>/dev/null && echo SEEN-STATE\n\
        echo \"lasting: $(ls -A \"$H/.hermes\" | tr '\\n' ' ')\"\n\
        cat \"$H/.hermes/hermes-agent/run.sh\" >/dev/null && echo CHECKOUT\n\
        cat \"$H/.hermes/tools/python-3/bin/python3\" >/dev/null && echo TOOLS\n\
        cat \"$H/.hermes/installs/k/facts.json\" >/dev/null && echo INSTALLS\n\
        cat \"$H/.local/bin/README\" >/dev/null && echo PROGRAM\n\
        echo \"lamp: $(ls -A \"$W/lamp\" | tr '\\n' ' ')\"\n\
        cat \"$W/lamp/AGENTS.md\" 2>/dev/null && echo SEEN-PROJECT\n\
        touch \"$W/lamp/x\" 2>/dev/null && echo WROTE-LAMP\n\
        touch \"$W/x\" 2>/dev/null && echo WROTE-W\n\
        touch \"$H/x\" 2>/dev/null && echo WROTE-H\n\
        touch \"$H/.hermes/x\" 2>/dev/null && echo WROTE-LASTING\n\
        touch \"$H/.hermes/hermes-agent/x\" 2>/dev/null && echo WROTE-CHECKOUT\n\
        touch \"$H/.hermes/tools/x\" 2>/dev/null && echo WROTE-TOOLS\n\
        touch \"$H/.hermes/installs/x\" 2>/dev/null && echo WROTE-INSTALLS\n\
        touch answer.md && touch \"$HOME/.hermes/memory.md\" && echo ANSWERED\n";
    // On the lamp, Hermes's home and the work folder are mounts by now
    // (moved from the staging); here they are made so, for `ro` to find.
    let b = base.display();
    let lines = make_lines(&base, checks).replacen(
        &format!("W={b}/w\n"),
        &format!("W={b}/w\nmount --bind {b}/h {b}/h && mount --bind {b}/w {b}/w || exit 1\n"),
        1,
    );
    assert!(lines.contains("mount --bind"));
    let script = base.join("make-lines");
    fs::write(&script, lines).unwrap();
    let out = std::process::Command::new("unshare")
        .args([
            "--map-root-user",
            "--mount",
            "--propagation",
            "private",
            "/bin/sh",
        ])
        .arg(&script)
        .current_dir(&make)
        .env(HOME_ENV, make.join("home"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "lasting: hermes-agent installs tools \nCHECKOUT\nTOOLS\nINSTALLS\nPROGRAM\nlamp: make-task-1 \nANSWERED\n"
    );
    // Outside, the make's answer is there and nothing else was touched.
    assert!(make.join("answer.md").exists());
    assert!(!base.join("w/lamp/x").exists() && !base.join("h/x").exists());
}
