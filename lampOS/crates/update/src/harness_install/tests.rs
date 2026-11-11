use super::*;
use crate::judge::UnitState;
use std::{
    cell::RefCell,
    os::unix::fs::{MetadataExt, symlink},
    path::PathBuf,
    rc::Rc,
};

/// What a round asked systemd, in order.
#[derive(Default)]
struct Fake {
    asked: Vec<&'static str>,
    start: Option<Result<(), String>>,
}

impl Systemd for Fake {
    fn states(&mut self, _: &[String]) -> Result<Vec<UnitState>, String> {
        unreachable!("Harness's unit asks no unit states")
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

const MINUTE_MS: u64 = 60_000;

/// A lamp: its root, with the units' folder and lamp-update's records.
struct Lamp {
    root: tempfile::TempDir,
}

impl Lamp {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("etc/systemd/system")).unwrap();
        fs::create_dir_all(root.path().join("usr/local/bin")).unwrap();
        fs::create_dir_all(root.path().join("var/lib/lampos-update")).unwrap();
        Self { root }
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
    fn records(&self) -> PathBuf {
        self.root.path().join("var/lib/lampos-update")
    }
    /// Harness's link and the file it names, both under the root.
    fn harness_there(&self) {
        let target = self.root.path().join("var/lib/lampos-tasks/.local/bin");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("harness"), "").unwrap();
        self.harness_link();
    }
    /// Harness's link alone: dangling unless its target is there too.
    fn harness_link(&self) {
        symlink(
            "/var/lib/lampos-tasks/.local/bin/harness",
            self.root.path().join("usr/local/bin/harness"),
        )
        .unwrap();
    }
    fn said(&self) -> Option<HarnessInstall> {
        record(&self.records())
    }
    fn unit_written(&self) -> bool {
        self.root
            .path()
            .join("etc/systemd/system")
            .join(UNIT)
            .exists()
    }
}

/// A runner that logs each command and answers it: `id -u` with 990,
/// `harness version` with `version`, `curl` with `manifest`, and the
/// installer with `installer`; the rest with nothing.
#[derive(Clone)]
struct Runs {
    log: Rc<RefCell<Vec<String>>>,
    installer: Result<String, String>,
    manifest: Result<String, String>,
    id: Result<String, String>,
    /// lamp-tasks' unit's `Environment`, as `systemctl show` says it.
    tasks_environment: String,
    /// lamp-update's own `ExecStart`, as `systemctl show` says it: the
    /// feed it follows (#6300). Empty unless set: not read, the hold kept.
    update_exec_start: String,
}

impl Runs {
    fn new() -> Self {
        Self {
            log: Rc::default(),
            installer: Ok(String::new()),
            manifest: Ok(String::new()),
            id: Ok("990\n".into()),
            tasks_environment: String::new(),
            update_exec_start: String::new(),
        }
    }
    fn runner(&self) -> impl FnMut(&[String]) -> Result<String, String> + '_ {
        move |argv: &[String]| {
            let line = argv.join(" ");
            self.log.borrow_mut().push(line.clone());
            if argv[0] == "id" {
                self.id.clone()
            } else if argv[0] == "curl" {
                self.manifest.clone()
            } else if install::runs_installer(argv) {
                self.installer.clone()
            } else if line.starts_with("systemctl show lamp-tasks.service") {
                Ok(self.tasks_environment.clone())
            } else if line.starts_with("systemctl show lamp-update.service") {
                Ok(self.update_exec_start.clone())
            } else if line.ends_with("harness version") {
                Ok("harness 0.4.25\n".into())
            } else {
                Ok(String::new())
            }
        }
    }
    fn ran(&self, part: &str) -> bool {
        self.log.borrow().iter().any(|line| line.contains(part))
    }
}

fn quiet() -> impl FnMut(&str) {
    |_: &str| {}
}

fn install(lamp: &Lamp, runs: &Runs, at: u64) -> HarnessInstall {
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

/// With the switch off, `serve` takes away an earlier record and writes
/// nothing, so `update/status` says no `harness`.
#[test]
fn the_switch_off_changes_nothing_and_says_no_harness() {
    let lamp = Lamp::new();
    at_start(lamp.root(), &lamp.records(), false, 1);
    assert_eq!(fs::read_dir(lamp.records()).unwrap().count(), 0);
    fs::write(
        lamp.records().join(RECORD),
        serde_json::to_vec(&HarnessInstall::new("failed", Some("installer"), 1)).unwrap(),
    )
    .unwrap();
    assert!(lamp.said().is_some());
    at_start(lamp.root(), &lamp.records(), false, 2);
    assert_eq!(lamp.said(), None);
    assert_eq!(fs::read_dir(lamp.records()).unwrap().count(), 0);
    assert_eq!(
        crate::status::status(&lamp.records(), "idle", None, None).harness,
        None
    );
}

/// Missing: `absent` at start (revision 1), then a round starts the one-shot
/// without waiting and says `installing`; a round soon after starts no
/// second; the one-shot installs, reads the version and starts lamp-tasks.
/// No Harness at boot and no daemon: no `service install`, no `start`.
#[test]
fn the_switch_on_installs_harness_when_missing_and_never_starts_its_daemon() {
    let lamp = Lamp::new();
    at_start(lamp.root(), &lamp.records(), true, 1);
    assert_eq!(lamp.said().unwrap().install, "absent");
    assert_eq!(
        crate::status::status(&lamp.records(), "idle", None, None)
            .harness
            .unwrap()
            .install,
        "absent"
    );

    let mut systemd = Fake::default();
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        2,
        &mut quiet(),
    )
    .unwrap();
    assert_eq!(
        (said.install.as_str(), said.pinned),
        ("installing", Some(false))
    );
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
    // Each change one revision on; a read never bumps it.
    assert_eq!(said.revision, 2);
    for _ in 0..2 {
        let read = crate::status::status(&lamp.records(), "idle", None, None);
        assert_eq!(read.harness.unwrap().revision, 2);
    }
    assert!(lamp.unit_written());
    assert_eq!(
        fs::read_to_string(lamp.root().join("etc/systemd/system").join(UNIT)).unwrap(),
        TEXT
    );
    let again = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        2 + MINUTE_MS,
        &mut quiet(),
    );
    assert_eq!(again, None, "a try under way is not started twice");
    assert_eq!(systemd.asked, ["reload", "show", "start"]);

    let runs = Runs::new();
    let said = install(&lamp, &runs, 3);
    assert_eq!(
        said,
        HarnessInstall {
            revision: 3,
            version: Some("0.4.25".into()),
            pinned: Some(false),
            ..HarnessInstall::new("installed", None, 3)
        }
    );
    assert_eq!(lamp.said(), Some(said));
    let log = runs.log.borrow().clone();
    let installer = log
        .iter()
        .find(|line| line.contains(install::HARNESS_INSTALLER))
        .unwrap();
    assert!(installer.starts_with("runuser -u lampos -- env HOME=/var/lib/lampos-tasks"));
    assert!(!installer.contains("HARNESS_METADATA_URL"), "{installer}");
    assert!(
        log.iter().any(|line| line
            == "ln -sfn /var/lib/lampos-tasks/.local/bin/harness /usr/local/bin/harness")
    );
    assert_eq!(log.last().unwrap(), "systemctl restart lamp-tasks.service");
    for never in ["service install", "harness start", "enable-linger"] {
        assert!(!runs.ran(never), "{never}: {log:?}");
    }
}

/// Harness already there: recorded `installed` once, nothing started, and
/// the one-shot (if it runs anyway) installs nothing.
#[test]
fn the_switch_on_skips_a_harness_already_there() {
    let lamp = Lamp::new();
    lamp.harness_there();
    at_start(lamp.root(), &lamp.records(), true, 1);
    assert_eq!(lamp.said(), None, "not missing: no absent");
    let mut systemd = Fake::default();
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        2,
        &mut quiet(),
    );
    assert_eq!(said.unwrap().install, "installed");
    assert!(systemd.asked.is_empty());
    assert!(!lamp.unit_written());
    assert_eq!(
        after_round(
            lamp.root(),
            &lamp.records(),
            &Ok(None),
            &mut systemd,
            3,
            &mut quiet()
        ),
        None
    );
    let runs = Runs::new();
    let said = install(&lamp, &runs, 4);
    assert_eq!(said.version.as_deref(), Some("0.4.25"));
    assert!(!runs.ran(install::HARNESS_INSTALLER));
    assert!(!runs.ran("ln -sfn"));
}

/// A link at Harness's place whose target is gone (#5241's reader) is
/// missing, not installed: `absent` at start, and a round starts the
/// one-shot, whose installer links it again.
#[test]
fn a_dangling_harness_link_counts_as_missing() {
    let lamp = Lamp::new();
    lamp.harness_link();
    at_start(lamp.root(), &lamp.records(), true, 1);
    assert_eq!(lamp.said().unwrap().install, "absent");
    let mut systemd = Fake::default();
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        2,
        &mut quiet(),
    );
    assert_eq!(said.unwrap().install, "installing");
    assert!(lamp.unit_written());
    let runs = Runs::new();
    assert_eq!(install(&lamp, &runs, 3).install, "installed");
    assert!(runs.ran(install::HARNESS_INSTALLER));
    assert!(runs.ran("ln -sfn"));
}

/// A failed install is recorded with its fixed reason, never its text, and
/// returns: nothing waits on it. The next round (or the app's check, which
/// starts one) tries again; so does an `installing` left past its bound.
#[test]
fn a_failed_install_is_recorded_without_blocking_and_tried_again() {
    let lamp = Lamp::new();
    let mut systemd = Fake::default();
    after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        1,
        &mut quiet(),
    );
    let mut runs = Runs::new();
    runs.installer = Err("runuser … sh -c curl …: exit status: 6 (/var/lib/x)".into());
    let said = install(&lamp, &runs, 2);
    assert_eq!(
        (said.install.as_str(), said.error.as_deref()),
        ("failed", Some("installer"))
    );
    assert!(!runs.ran("ln -sfn"), "no link to nothing");
    assert!(!runs.ran("lamp-tasks.service"));
    let text = fs::read_to_string(lamp.records().join(RECORD)).unwrap();
    assert!(!text.contains("/var/lib/x"), "{text}");
    let status = crate::status::status(&lamp.records(), "idle", None, None);
    assert_eq!(status.harness.unwrap().error.as_deref(), Some("installer"));

    // The retry: the first round past the backoff starts the one-shot
    // again (a_deterministic_failure_waits_out_its_backoff).
    let retried = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        2 + RETRY_AFTER.as_millis() as u64,
        &mut quiet(),
    );
    assert_eq!(retried.unwrap().install, "installing");
    assert_eq!(
        systemd
            .asked
            .iter()
            .filter(|asked| **asked == "start")
            .count(),
        2
    );
    let runs = Runs::new();
    assert_eq!(install(&lamp, &runs, 4).install, "installed");

    // An `installing` left behind is tried again past its bound only.
    let lamp = Lamp::new();
    let mut systemd = Fake::default();
    after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        1,
        &mut quiet(),
    );
    let stuck = STUCK_AFTER.as_millis() as u64;
    assert_eq!(
        after_round(
            lamp.root(),
            &lamp.records(),
            &Ok(None),
            &mut systemd,
            stuck,
            &mut quiet()
        ),
        None
    );
    let tried = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        stuck + 1,
        &mut quiet(),
    );
    assert_eq!(tried.unwrap().at_unix_ms, stuck + 1);

    // A unit systemd won't start is `not_started`.
    let lamp = Lamp::new();
    let mut systemd = Fake {
        start: Some(Err("Unit not found".into())),
        ..Fake::default()
    };
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        1,
        &mut quiet(),
    );
    assert_eq!(said.unwrap().error.as_deref(), Some("not_started"));

    // Past the installer's bound, and with no `lampos` user.
    for (runs, reason) in [
        (
            Runs {
                installer: Err("sh: did not end within 900 s: it ran on".into()),
                ..Runs::new()
            },
            "timeout",
        ),
        (
            Runs {
                id: Err("id -u lampos: no such user".into()),
                ..Runs::new()
            },
            "no_user",
        ),
    ] {
        let lamp = Lamp::new();
        after_round(
            lamp.root(),
            &lamp.records(),
            &Ok(None),
            &mut Fake::default(),
            1,
            &mut quiet(),
        );
        assert_eq!(install(&lamp, &runs, 2).error.as_deref(), Some(reason));
    }
}

/// A failure that the same steps would only repeat, `installer` or
/// `no_user` (#5241's reader), is not tried after every round: the rounds
/// within [`RETRY_AFTER`] of it start nothing, and the first past it tries
/// again. One that may pass at once, `timeout` here, is tried at the next.
#[test]
fn a_deterministic_failure_waits_out_its_backoff() {
    for (runs, reason, held) in [
        (
            Runs {
                installer: Err("sh -c curl …: exit status: 1".into()),
                ..Runs::new()
            },
            "installer",
            true,
        ),
        (
            Runs {
                id: Err("id -u lampos: no such user".into()),
                ..Runs::new()
            },
            "no_user",
            true,
        ),
        (
            Runs {
                installer: Err("sh: did not end within 900 s: it ran on".into()),
                ..Runs::new()
            },
            "timeout",
            false,
        ),
    ] {
        let lamp = Lamp::new();
        let mut systemd = Fake::default();
        let round = |systemd: &mut Fake, at: u64| {
            after_round(
                lamp.root(),
                &lamp.records(),
                &Ok(None),
                systemd,
                at,
                &mut quiet(),
            )
        };
        round(&mut systemd, 1);
        let failed_at = 10 * MINUTE_MS;
        assert_eq!(
            install(&lamp, &runs, failed_at).error.as_deref(),
            Some(reason)
        );
        let starts = |systemd: &Fake| systemd.asked.iter().filter(|a| **a == "start").count();
        let soon = round(&mut systemd, failed_at + MINUTE_MS);
        if held {
            assert_eq!(soon, None, "{reason}: held");
            let edge = failed_at + RETRY_AFTER.as_millis() as u64 - 1;
            assert_eq!(round(&mut systemd, edge), None, "{reason}: held to its end");
            assert_eq!(starts(&systemd), 1, "{reason}");
            let past = round(&mut systemd, edge + 1);
            assert_eq!(past.unwrap().install, "installing", "{reason}: past it");
        } else {
            assert_eq!(soon.unwrap().install, "installing", "{reason}: at once");
        }
        assert_eq!(starts(&systemd), 2, "{reason}");
    }
}

/// Pinned: the manifest is fetched and its sha256 checked before anything
/// is installed; one that differs installs nothing; a match installs
/// through `HARNESS_METADATA_URL`, at the checked bytes served on
/// 127.0.0.1. A pin set wrongly starts nothing.
#[test]
fn a_pinned_manifest_with_another_sha256_is_refused() {
    let url = "https://example.invalid/harness/manifest.json";
    let manifest = "{\"version\":\"0.4.25\"}";
    let right = crate::manifest::sha256_hex(manifest.as_bytes());
    let pin = |sha256: &str| pin_from(Some(url), Some(sha256));

    let lamp = Lamp::new();
    let wrong = "0".repeat(64);
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &pin(&wrong),
        &mut Fake::default(),
        1,
        &mut quiet(),
    );
    assert_eq!(said.unwrap().pinned, Some(true));
    let runs = Runs {
        manifest: Ok(manifest.into()),
        ..Runs::new()
    };
    let said = install(&lamp, &runs, 2);
    assert_eq!(said.error.as_deref(), Some("pin_mismatch"));
    assert_eq!(said.pinned, Some(true));
    assert!(runs.ran(&format!(
        "curl -fsSL --proto =https --tlsv1.2 --max-filesize 1048576 {url}"
    )));
    assert!(!runs.ran(install::HARNESS_INSTALLER), "nothing installed");
    assert!(!runs.ran(install::PINNED_INSTALLER), "nothing installed");

    let lamp = Lamp::new();
    after_round(
        lamp.root(),
        &lamp.records(),
        &pin(&right.to_ascii_uppercase()),
        &mut Fake::default(),
        1,
        &mut quiet(),
    );
    let said = install(&lamp, &runs, 3);
    assert_eq!(
        (said.install.as_str(), said.pinned),
        ("installed", Some(true))
    );
    let log = runs.log.borrow().clone();
    let installer = log
        .iter()
        .rev()
        .find(|line| line.contains(install::PINNED_INSTALLER))
        .unwrap();
    assert!(
        !installer.contains(install::HARNESS_INSTALLER),
        "{installer}"
    );
    // The checked bytes, from lamp-update's own listener (#5229), never the
    // URL again.
    assert!(
        installer.contains("HARNESS_METADATA_URL=http://127.0.0.1:"),
        "{installer}"
    );
    assert!(!installer.contains(url), "{installer}");

    let lamp = Lamp::new();
    let runs = Runs {
        manifest: Err("curl: (22) 404".into()),
        ..Runs::new()
    };
    after_round(
        lamp.root(),
        &lamp.records(),
        &pin(&right),
        &mut Fake::default(),
        1,
        &mut quiet(),
    );
    assert_eq!(install(&lamp, &runs, 2).error.as_deref(), Some("pin_fetch"));

    for (url, sha256) in [
        (Some(url), None),
        (None, Some(right.as_str())),
        (Some("http://example.invalid/m.json"), Some(right.as_str())),
        (Some(url), Some("abc")),
    ] {
        let lamp = Lamp::new();
        let mut systemd = Fake::default();
        let said = after_round(
            lamp.root(),
            &lamp.records(),
            &pin_from(url, sha256),
            &mut systemd,
            1,
            &mut quiet(),
        );
        assert_eq!(said.unwrap().error.as_deref(), Some("pin_unreadable"));
        assert!(systemd.asked.is_empty());
    }
    assert_eq!(pin_from(None, None), Ok(None));
}

/// One install at a time: a second one-shot is refused, writing nothing.
#[test]
fn a_second_install_waits_for_none() {
    let lamp = Lamp::new();
    after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut Fake::default(),
        1,
        &mut quiet(),
    );
    let _held = lock(&lamp.records()).unwrap();
    let runs = Runs::new();
    let mut run = runs.runner();
    let refused = install_now(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 2,
        &mut quiet(),
    );
    assert!(refused.is_err());
    assert_eq!(lamp.said().unwrap().install, "installing");
    assert!(runs.log.borrow().is_empty());
}

/// The build's own pin (#5229): nothing fetched; the installer is given
/// lamp-update's listener on 127.0.0.1, which serves exactly the compiled-in
/// manifest while the installer runs, and `harness version` is compared
/// with the version it pins.
#[test]
fn the_builds_own_pin_is_served_to_the_installer_from_loopback() {
    let lamp = Lamp::new();
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(Some(product::built_in())),
        &mut Fake::default(),
        1,
        &mut quiet(),
    );
    assert_eq!(said.unwrap().pinned, Some(true));
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let served: Rc<RefCell<Option<String>>> = Rc::default();
    let mut run = |argv: &[String]| -> Result<String, String> {
        let line = argv.join(" ");
        log.borrow_mut().push(line.clone());
        if argv[0] == "id" {
            return Ok("990\n".into());
        }
        if install::runs_installer(argv) {
            let url = argv
                .iter()
                .find_map(|arg| arg.strip_prefix("HARNESS_METADATA_URL="))
                .unwrap();
            *served.borrow_mut() = Some(fetch(url));
        }
        if line.ends_with("harness version") {
            return Ok(format!("harness {}\n", product::VERSION));
        }
        Ok(String::new())
    };
    let mut lines = Vec::new();
    let said = install_now(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 2,
        &mut |line: &str| lines.push(line.to_owned()),
    )
    .unwrap();
    assert_eq!(
        (said.install.as_str(), said.pinned, said.version.as_deref()),
        ("installed", Some(true), Some(product::VERSION))
    );
    assert!(!log.borrow().iter().any(|line| line.starts_with("curl ")));
    let body = served.borrow().clone().unwrap();
    assert!(body.starts_with("HTTP/1.1 200 OK\r\n"), "{body}");
    assert!(
        body.as_bytes().ends_with(product::MANIFEST),
        "the installer read the compiled-in bytes: {body}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == &format!("Harness {} is the pinned version", product::VERSION)),
        "{lines:?}"
    );
}

/// The build's own pin with bytes that are not its sha256 installs nothing:
/// the check runs before each install, not only in the test of the copy.
#[test]
fn a_built_in_pin_with_another_sha256_installs_nothing() {
    let lamp = Lamp::new();
    let pin = Pin {
        sha256: "0".repeat(64),
        ..product::built_in()
    };
    after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(Some(pin)),
        &mut Fake::default(),
        1,
        &mut quiet(),
    );
    let runs = Runs::new();
    let said = install(&lamp, &runs, 2);
    assert_eq!(said.error.as_deref(), Some("pin_mismatch"));
    assert!(!runs.ran("curl "));
    assert!(!runs.ran(install::HARNESS_INSTALLER));
    assert!(!runs.ran(install::PINNED_INSTALLER));
}

/// What the installer's run was handed: its file's bytes and mode, and
/// each `HARNESS_*` variable with what its URL served.
type Seen = (Vec<u8>, u32, Vec<(String, String)>);

/// The installer a pinned one-shot runs (#5229) is this build's checked
/// copy, run from a file (`sh <file>`, no CDN URL, no pipe), readable by
/// `lampos` and taken away after; every manifest it reads is a loopback
/// URL serving the compiled-in bytes; `HARNESS_INSTALL_ATTENDED` is taken
/// out, so its OpenCode step never runs. Both pins: the build's own and
/// one in the environment.
#[test]
fn a_pinned_install_runs_the_builds_installer_from_a_file_with_every_manifest_on_loopback() {
    let url = "https://example.invalid/harness/manifest.json";
    let fetched_manifest = "{\"cli\":{\"version\":\"0.4.25\"}}";
    let fetched_pin = pin_from(
        Some(url),
        Some(&crate::manifest::sha256_hex(fetched_manifest.as_bytes())),
    )
    .unwrap()
    .unwrap();
    for (pin, cli, want) in [
        (
            product::built_in(),
            product::MANIFEST.to_vec(),
            product::VERSION,
        ),
        (fetched_pin, fetched_manifest.as_bytes().to_vec(), "0.4.25"),
    ] {
        let lamp = Lamp::new();
        after_round(
            lamp.root(),
            &lamp.records(),
            &Ok(Some(pin.clone())),
            &mut Fake::default(),
            1,
            &mut quiet(),
        );
        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let seen: Rc<RefCell<Option<Seen>>> = Rc::default();
        let file: Rc<RefCell<Option<PathBuf>>> = Rc::default();
        let mut run = |argv: &[String]| -> Result<String, String> {
            log.borrow_mut().push(argv.join(" "));
            if argv[0] == "id" {
                return Ok("990\n".into());
            }
            if argv[0] == "curl" {
                return Ok(fetched_manifest.into());
            }
            if argv.last().is_some_and(|last| last == "version") {
                return Ok(format!("harness {want}\n"));
            }
            if install::runs_installer(argv) {
                use std::os::unix::fs::PermissionsExt;
                let path = PathBuf::from(argv.last().unwrap());
                let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                let bodies = argv
                    .iter()
                    .filter_map(|arg| {
                        let (name, value) = arg.split_once('=')?;
                        name.starts_with("HARNESS_")
                            .then(|| (name.to_owned(), fetch(value)))
                    })
                    .collect();
                *seen.borrow_mut() = Some((fs::read(&path).unwrap(), mode, bodies));
                *file.borrow_mut() = Some(path);
            }
            Ok(String::new())
        };
        let said = install_now(
            lamp.root(),
            &lamp.records(),
            &mut run,
            &mut || 2,
            &mut quiet(),
        )
        .unwrap();
        assert_eq!(said.install, "installed", "{pin:?}");
        let log = log.borrow().clone();
        let line = log
            .iter()
            .find(|line| line.contains(install::PINNED_INSTALLER))
            .unwrap();
        assert!(
            line.starts_with(
                "runuser -u lampos -- env -u HARNESS_INSTALL_ATTENDED HOME=/var/lib/lampos-tasks "
            ),
            "{line}"
        );
        assert!(!line.contains(install::HARNESS_INSTALLER), "{line}");
        assert!(!line.contains("| sh") && !line.contains(" -c "), "{line}");
        assert!(!line.contains("HARNESS_INSTALL_ATTENDED="), "{line}");
        assert!(
            !log.iter()
                .any(|line| line.contains(install::HARNESS_INSTALLER))
        );
        // Only an environment pin's manifest is fetched, and nothing else.
        let fetches: Vec<_> = log
            .iter()
            .filter(|line| line.starts_with("curl "))
            .collect();
        assert_eq!(fetches.len(), usize::from(!pin.built_in), "{fetches:?}");

        let (bytes, mode, bodies) = seen.borrow_mut().take().unwrap();
        assert_eq!(bytes, product::INSTALLER, "the build's installer ran");
        assert_eq!(mode, 0o644, "lampos reads it, only root writes it");
        let mut wanted = vec![("HARNESS_METADATA_URL".to_owned(), cli.clone())];
        wanted.extend(
            product::MANIFESTS
                .iter()
                .map(|manifest| (manifest.env.to_owned(), manifest.bytes.to_vec())),
        );
        assert_eq!(bodies.len(), wanted.len(), "{bodies:?}");
        for ((name, body), (want_name, want)) in bodies.iter().zip(&wanted) {
            assert_eq!(name, want_name);
            assert!(body.starts_with("HTTP/1.1 200 OK\r\n"), "{name}: {body}");
            assert!(body.as_bytes().ends_with(want), "{name}: {body}");
        }
        for name in ["HARNESS_METADATA_URL", "HARNESS_TUI_MANIFEST_URL"] {
            assert!(
                line.contains(&format!("{name}=http://127.0.0.1:")),
                "{name}: {line}"
            );
        }
        let path = file.borrow().clone().unwrap();
        assert!(!path.exists(), "the installer's file is taken away after");
        assert!(!path.parent().unwrap().exists());
    }
}

/// Without a pin (a builder's lamp, `harness-on`), the installer is still
/// Harness's CDN one piped to `sh`, exactly as before #5229, with nothing
/// served and nothing taken out of its environment.
#[test]
fn an_unpinned_install_still_pipes_the_cdn_installer() {
    let mut log = Vec::new();
    let mut run = |argv: &[String]| -> Result<String, String> {
        log.push(argv.to_vec());
        Ok(if argv[0] == "id" {
            "990\n".into()
        } else {
            String::new()
        })
    };
    install::harness_on_with(&mut run, USER, HARNESS_HOME).unwrap();
    let installer = log
        .iter()
        .find(|argv| install::runs_installer(argv))
        .unwrap();
    assert_eq!(
        installer.join(" "),
        format!(
            "runuser -u lampos -- env HOME=/var/lib/lampos-tasks XDG_RUNTIME_DIR=/run/user/990 PATH=/usr/local/bin:/usr/bin:/bin SHELL=/bin/sh sh -c curl -fsSL --proto =https --tlsv1.2 {} | sh",
            install::HARNESS_INSTALLER
        )
    );
}

/// The installer's bound ([`install::INSTALLER_WITHIN`]) holds for both
/// runs: the CDN pipe and a pinned file. Other `sh` runs are not it.
#[test]
fn both_installer_runs_get_the_installers_bound() {
    let argv = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let piped = format!("curl -fsSL {} | sh", install::HARNESS_INSTALLER);
    assert!(install::runs_installer(&argv(&[
        "runuser", "sh", "-c", &piped
    ])));
    let file = format!("/tmp/lamp-update-harness-1-2/{}", install::PINNED_INSTALLER);
    assert!(install::runs_installer(&argv(&[
        "runuser", "env", "sh", &file
    ])));
    assert!(!install::runs_installer(&argv(&["sh", "/tmp/other.sh"])));
    assert!(!install::runs_installer(&argv(&["cat", &file])));
    assert!(!install::runs_installer(&[]));
}

/// A tampered byte in the installer or in any manifest the build pins is
/// `pin_mismatch`: nothing is served, written or run.
#[test]
fn a_tampered_installer_or_manifest_installs_nothing() {
    let mut kits = vec![Kit {
        installer: b"#!/bin/sh\ncurl https://example.invalid/x | sh\n",
        ..Kit::BUILT
    }];
    for at in 0..product::MANIFESTS.len() {
        let mut manifests = product::MANIFESTS;
        manifests[at].bytes = b"{\"tampered\":true}";
        kits.push(Kit {
            manifests,
            ..Kit::BUILT
        });
    }
    for kit in kits {
        let lamp = Lamp::new();
        after_round(
            lamp.root(),
            &lamp.records(),
            &Ok(Some(product::built_in())),
            &mut Fake::default(),
            1,
            &mut quiet(),
        );
        let runs = Runs::new();
        let mut run = runs.runner();
        let said = install_with(
            lamp.root(),
            &lamp.records(),
            &mut run,
            &mut || 2,
            &mut quiet(),
            &kit,
        )
        .unwrap();
        assert_eq!(said.error.as_deref(), Some("pin_mismatch"));
        let log = runs.log.borrow().clone();
        assert!(
            !log.iter()
                .any(|line| line.starts_with("runuser") || line.starts_with("install ")),
            "{log:?}"
        );
    }
}

/// A listener lamp-update cannot make on 127.0.0.1 is `serve`, not
/// `installer` (#5229's reader): nothing runs, and the next round tries
/// again at once, with no [`RETRY_AFTER`] wait.
#[test]
fn a_listener_not_made_is_serve_and_tried_at_the_next_round() {
    let kit = Kit {
        bind: || Err(io::Error::new(ErrorKind::AddrNotAvailable, "no loopback")),
        ..Kit::BUILT
    };
    let lamp = Lamp::new();
    let mut systemd = Fake::default();
    let pin = Ok(Some(product::built_in()));
    after_round(
        lamp.root(),
        &lamp.records(),
        &pin,
        &mut systemd,
        1,
        &mut quiet(),
    );
    let runs = Runs::new();
    let mut run = runs.runner();
    let said = install_with(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 10 * MINUTE_MS,
        &mut quiet(),
        &kit,
    )
    .unwrap();
    assert_eq!(said.error.as_deref(), Some("serve"));
    assert!(said.check(), "lamp-cloud passes it");
    assert!(!runs.ran("runuser"), "nothing ran");
    let next = after_round(
        lamp.root(),
        &lamp.records(),
        &pin,
        &mut systemd,
        11 * MINUTE_MS,
        &mut quiet(),
    );
    assert_eq!(next.unwrap().install, "installing", "not held");
}

/// A `GET` of `url` on loopback, the whole answer.
fn fetch(url: &str) -> String {
    use std::io::{Read, Write};
    let rest = url.strip_prefix("http://").unwrap();
    let (address, path) = rest.split_once('/').unwrap();
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    write!(stream, "GET /{path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
    let mut said = String::new();
    stream.read_to_string(&mut said).unwrap();
    said
}

/// A pinned install whose `harness version` is not the pinned one (or says
/// none) is recorded `failed` with `version`, not `installed` (#5229's
/// reader), so the app tells it apart; it stays so at the next rounds while
/// Harness is there, and is never held by [`RETRY_AFTER`].
#[test]
fn a_pinned_install_reporting_another_version_records_version() {
    for answer in [
        Ok("harness 0.5.40\n".to_owned()),
        Err("exit status: 1".to_owned()),
    ] {
        let lamp = Lamp::new();
        let mut systemd = Fake::default();
        let pin = Ok(Some(product::built_in()));
        after_round(
            lamp.root(),
            &lamp.records(),
            &pin,
            &mut systemd,
            1,
            &mut quiet(),
        );
        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut run = |argv: &[String]| -> Result<String, String> {
            log.borrow_mut().push(argv.join(" "));
            if argv[0] == "id" {
                Ok("990\n".into())
            } else if argv.last().is_some_and(|last| last == "version") {
                answer.clone()
            } else {
                Ok(String::new())
            }
        };
        let mut lines = Vec::new();
        let said = install_now(
            lamp.root(),
            &lamp.records(),
            &mut run,
            &mut || 2,
            &mut |line: &str| lines.push(line.to_owned()),
        )
        .unwrap();
        assert_eq!(
            (said.install.as_str(), said.error.as_deref(), said.pinned),
            ("failed", Some("version"), Some(true)),
            "{answer:?}"
        );
        assert!(said.check(), "lamp-cloud passes it");
        assert_ne!(said.install, "installed");
        assert!(
            lines
                .iter()
                .any(|line| line.ends_with(&format!(", not the pinned {}", product::VERSION))),
            "{lines:?}"
        );
        // Harness is there: the next round keeps `version`, starting nothing.
        lamp.harness_there();
        let next = after_round(
            lamp.root(),
            &lamp.records(),
            &pin,
            &mut systemd,
            3,
            &mut quiet(),
        );
        assert_eq!(next, None);
        assert_eq!(lamp.said().unwrap().error.as_deref(), Some("version"));
        assert_eq!(systemd.asked.iter().filter(|a| **a == "start").count(), 1);
    }
    // Unpinned, no version is pinned, so any is `installed`.
    let lamp = Lamp::new();
    after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut Fake::default(),
        1,
        &mut quiet(),
    );
    assert_eq!(install(&lamp, &Runs::new(), 2).install, "installed");
}

/// Harness's launcher as its installer writes it without `--no-updates`.
const UNPINNED: &str = "#!/bin/sh\nexec \"/var/lib/lampos-tasks/.local/share/harness/node/bin/node\" \"/var/lib/lampos-tasks/.local/share/harness/cli/0.5.39/cli.js\" \"$@\"\n";
/// The same in Harness's pinned shape (#5493).
const PINNED: &str = "#!/bin/sh\nexport ADAPTER_UPDATE_DISABLE=true\nexport HARNESS_ENV_FILE=/var/lib/lampos-tasks/harness-hold.env\nexec \"/var/lib/lampos-tasks/.local/share/harness/node/bin/node\" \"/var/lib/lampos-tasks/.local/share/harness/cli/0.5.39/cli.js\" \"$@\"\n";
const HOLD_ON: &str =
    "HOME=/var/lib/lampos-tasks PATH=/usr/local/bin:/usr/bin:/bin LAMP_HARNESS_HOLD_UPDATES=on";

/// The launcher the (fake) installer left, with no link at Harness's
/// place yet, so the one-shot installs.
fn launcher_left(lamp: &Lamp, text: &str) -> PathBuf {
    let bin = lamp.root().join("var/lib/lampos-tasks/.local/bin");
    fs::create_dir_all(&bin).unwrap();
    let launcher = bin.join("harness");
    fs::write(&launcher, text).unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
    launcher
}

fn asked_for_install(lamp: &Lamp) {
    write_whole(
        &lamp.records().join(ASK),
        &serde_json::to_vec(&Ask {
            version: 1,
            pin: None,
            upgrade_from: None,
        })
        .unwrap(),
        0o600,
    )
    .unwrap();
}

/// With lamp-tasks holding Harness's updates, the install writes Harness's
/// launcher in its pinned shape before lamp-tasks starts, keeping its exec
/// line and its mode; a second install keeps it byte for byte.
#[test]
fn with_the_hold_on_the_install_pins_harnesss_launcher_once() {
    let lamp = Lamp::new();
    let launcher = launcher_left(&lamp, UNPINNED);
    asked_for_install(&lamp);
    let mut runs = Runs::new();
    runs.tasks_environment = HOLD_ON.into();
    let mut lines = Vec::new();
    let said = {
        let mut run = runs.runner();
        install_now(
            lamp.root(),
            &lamp.records(),
            &mut run,
            &mut || 3,
            &mut |line: &str| lines.push(line.to_owned()),
        )
        .unwrap()
    };
    assert_eq!(said.install, "installed");
    assert_eq!(fs::read_to_string(&launcher).unwrap(), PINNED);
    assert_eq!(
        fs::metadata(&launcher).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Harness's launcher pinned (")),
        "{lines:?}"
    );
    let log = runs.log.borrow().clone();
    let shown = log
        .iter()
        .position(|line| line.starts_with("systemctl show lamp-tasks.service"))
        .unwrap();
    assert!(shown < log.len() - 1, "before lamp-tasks starts: {log:?}");
    assert_eq!(log.last().unwrap(), "systemctl restart lamp-tasks.service");

    // Pinned already: the same bytes, no second preamble.
    assert_eq!(
        launcher::pin(&launcher, "/var/lib/lampos-tasks/harness-hold.env"),
        Ok(launcher::Pinned::Kept)
    );
    let mut run = runs.runner();
    let mut kept = Vec::new();
    launcher::after_install(lamp.root(), &mut run, &mut |line: &str| {
        kept.push(line.to_owned())
    });
    assert_eq!(kept, ["Harness's launcher already pinned"]);
    assert_eq!(fs::read_to_string(&launcher).unwrap(), PINNED);
    assert!(!launcher.with_file_name(".harness.lampos-pin").exists());
}

/// An exec line Harness's own update rewrote is kept as it is.
#[test]
fn the_launchers_own_exec_line_is_kept() {
    let updated = UNPINNED.replace("0.5.39", "0.5.57");
    let pinned =
        launcher::pinned_launcher(&updated, "/var/lib/lampos-tasks/harness-hold.env").unwrap();
    assert_eq!(pinned, PINNED.replace("0.5.39", "0.5.57"));
    assert_eq!(pinned.lines().last(), updated.lines().last());
    assert_eq!(
        launcher::pinned_launcher("#!/bin/sh\necho no exec\n", "/x"),
        None
    );
}

/// lamp-update's `ExecStart` following `feed`, as `systemctl show
/// --property=ExecStart --value` gives it.
fn exec_start(feed: &str) -> String {
    format!(
        "{{ path=/opt/lampOS/update/current/lamp-update ; argv[]=/opt/lampOS/update/current/lamp-update serve --channel-url {feed} --channel lamp-voice --key-file /etc/lampOS/update/key.pub --apply on ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}\n"
    )
}

/// An install with the hold on, lamp-update showing `exec_start`, from a
/// launcher left as `text`: the launcher after, and the lines said of it.
fn install_on_feed(exec_start: &str, text: &str) -> (String, Vec<String>) {
    let lamp = Lamp::new();
    let launcher = launcher_left(&lamp, text);
    asked_for_install(&lamp);
    let mut runs = Runs::new();
    runs.tasks_environment = HOLD_ON.into();
    runs.update_exec_start = exec_start.into();
    let mut lines = Vec::new();
    {
        let mut run = runs.runner();
        install_now(
            lamp.root(),
            &lamp.records(),
            &mut run,
            &mut || 3,
            &mut |line: &str| lines.push(line.to_owned()),
        )
        .unwrap();
    }
    let said = lines
        .into_iter()
        .filter(|line| line.contains("launcher"))
        .collect();
    (fs::read_to_string(&launcher).unwrap(), said)
}

/// #6300: on the stable feed, any other feed, or one not read, the install
/// pins the launcher exactly as before, from an unpinned launcher or a
/// pinned one.
#[test]
fn stable_and_an_unknown_feed_keep_the_launcher_pinned() {
    for shown in [
        exec_start(lamp_msgs::harness_hold::STABLE_FEED),
        exec_start("file:///var/tmp/lampos-channel/"),
        exec_start(
            "https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-11308/",
        ),
        String::new(),
    ] {
        let (launcher, said) = install_on_feed(&shown, UNPINNED);
        assert_eq!(launcher, PINNED, "{shown}");
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("Harness's launcher pinned ("),
            "{said:?}"
        );
        let (launcher, said) = install_on_feed(&shown, PINNED);
        assert_eq!(launcher, PINNED, "{shown}");
        assert_eq!(said, ["Harness's launcher already pinned"], "{shown}");
    }
}

/// #6300: on the lamp-voice feed (a dev or beta lamp, the canary), the
/// install takes the hold out of the launcher, its exec line kept, so a
/// Harness command run by hand lets Harness's own updater run.
#[test]
fn the_lamp_voice_feed_takes_the_hold_out_of_the_launcher() {
    let shown = exec_start(lamp_msgs::harness_hold::CANARY_FEED);
    let (launcher, said) = install_on_feed(&shown, PINNED);
    assert_eq!(launcher, UNPINNED);
    assert_eq!(
        said,
        [
            "Harness's launcher unpinned: this lamp follows the lamp-voice feed, the canary for Harness's releases (#6300)"
        ]
    );
    let (launcher, said) = install_on_feed(&shown, UNPINNED);
    assert_eq!(launcher, UNPINNED);
    assert_eq!(
        said,
        [
            "Harness's launcher not pinned: this lamp follows the lamp-voice feed, the canary for Harness's releases (#6300)"
        ]
    );
    let updated = PINNED.replace("0.5.39", "0.6.13");
    assert_eq!(
        launcher::unpinned_launcher(&updated).unwrap(),
        UNPINNED.replace("0.5.39", "0.6.13")
    );
}

/// With the hold off (unset, or anything but `on`), the install leaves the
/// launcher as it is.
#[test]
fn with_the_hold_off_the_install_leaves_the_launcher_alone() {
    for environment in [
        "",
        "HOME=/var/lib/lampos-tasks",
        "LAMP_HARNESS_HOLD_UPDATES=off",
        "LAMP_HARNESS_HOLD_UPDATES=ON",
    ] {
        let lamp = Lamp::new();
        let launcher = launcher_left(&lamp, UNPINNED);
        asked_for_install(&lamp);
        let mut runs = Runs::new();
        runs.tasks_environment = environment.into();
        runs.update_exec_start = exec_start(lamp_msgs::harness_hold::CANARY_FEED);
        let mut lines = Vec::new();
        let mut run = runs.runner();
        install_now(
            lamp.root(),
            &lamp.records(),
            &mut run,
            &mut || 3,
            &mut |line: &str| lines.push(line.to_owned()),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(&launcher).unwrap(),
            UNPINNED,
            "{environment}"
        );
        let said: Vec<_> = lines
            .iter()
            .filter(|line| line.contains("launcher"))
            .collect();
        assert_eq!(
            said,
            [
                "LAMP_HARNESS_HOLD_UPDATES is not on in lamp-tasks.service: Harness's launcher left as it is"
            ],
            "{environment}"
        );
    }
}

/// With the hold on and no launcher, none is made, and that is said.
#[test]
fn a_missing_launcher_is_not_made() {
    let lamp = Lamp::new();
    asked_for_install(&lamp);
    let mut runs = Runs::new();
    runs.tasks_environment = HOLD_ON.into();
    let mut lines = Vec::new();
    {
        let mut run = runs.runner();
        install_now(
            lamp.root(),
            &lamp.records(),
            &mut run,
            &mut || 3,
            &mut |line: &str| lines.push(line.to_owned()),
        )
        .unwrap();
    }
    assert!(
        lines.iter().any(|line| line
            == "no Harness launcher at /var/lib/lampos-tasks/.local/bin/harness: left alone"),
        "{lines:?}"
    );
    assert!(!lamp.root().join("var/lib/lampos-tasks/.local").exists());
}

/// lamp-tasks' unit not read (systemctl failed): taken as off, the launcher
/// left as it is, one line said.
#[test]
fn an_unread_tasks_unit_is_taken_as_off() {
    let lamp = Lamp::new();
    let launcher = launcher_left(&lamp, UNPINNED);
    let mut lines = Vec::new();
    let mut run = |argv: &[String]| -> Result<String, String> {
        assert_eq!(argv[0], "systemctl");
        Err("Failed to connect to bus".to_owned())
    };
    launcher::after_install(lamp.root(), &mut run, &mut |line: &str| {
        lines.push(line.to_owned())
    });
    assert_eq!(
        lines,
        [
            "lamp-tasks.service's environment not read (Failed to connect to bus): LAMP_HARNESS_HOLD_UPDATES taken as off, Harness's launcher left as it is"
        ]
    );
    assert_eq!(fs::read_to_string(&launcher).unwrap(), UNPINNED);
}

/// A link planted at the temporary name (the tasks user owns the bin
/// folder): the root-run pin never opens through it, so the file it points
/// at is not truncated, written, chowned or chmodded; the launcher is still
/// pinned, and a second run changes nothing.
#[test]
fn a_link_at_the_temporary_name_is_not_followed() {
    let lamp = Lamp::new();
    let launcher = launcher_left(&lamp, UNPINNED);
    let victim = lamp.root().join("victim");
    fs::write(&victim, "root's own\n").unwrap();
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o640)).unwrap();
    let next = launcher.with_file_name(".harness.lampos-pin");
    symlink(&victim, &next).unwrap();
    let env_file = "/var/lib/lampos-tasks/harness-hold.env";

    assert_eq!(
        launcher::pin(&launcher, env_file),
        Ok(launcher::Pinned::Written)
    );
    assert_eq!(fs::read_to_string(&victim).unwrap(), "root's own\n");
    assert_eq!(
        fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::read_to_string(&launcher).unwrap(), PINNED);
    assert!(
        !fs::symlink_metadata(&launcher)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::symlink_metadata(&next).is_err(),
        "the planted name is gone"
    );

    assert_eq!(
        launcher::pin(&launcher, env_file),
        Ok(launcher::Pinned::Kept)
    );
    assert_eq!(fs::read_to_string(&launcher).unwrap(), PINNED);
    assert_eq!(fs::read_to_string(&victim).unwrap(), "root's own\n");
}

/// A launcher that is itself a link: left as it is and said; its target is
/// neither read nor written.
#[test]
fn a_launcher_that_is_a_link_is_left_alone() {
    let lamp = Lamp::new();
    let bin = lamp.root().join("var/lib/lampos-tasks/.local/bin");
    fs::create_dir_all(&bin).unwrap();
    let target = lamp.root().join("elsewhere");
    fs::write(&target, UNPINNED).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    let launcher = bin.join("harness");
    symlink(&target, &launcher).unwrap();
    let mut runs = Runs::new();
    runs.tasks_environment = HOLD_ON.into();
    let mut lines = Vec::new();
    let mut run = runs.runner();
    launcher::after_install(lamp.root(), &mut run, &mut |line: &str| {
        lines.push(line.to_owned())
    });
    assert_eq!(
        lines,
        [format!(
            "Harness's launcher not pinned: {}: a link, left as it is (its target not read or written)",
            launcher.display()
        )]
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), UNPINNED);
    assert!(
        fs::symlink_metadata(&launcher)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!bin.join(".harness.lampos-pin").exists());
}

/// A link in place of the bin folder: not followed either.
#[test]
fn a_link_in_place_of_the_bin_folder_is_not_followed() {
    let lamp = Lamp::new();
    let elsewhere = lamp.root().join("elsewhere-bin");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("harness"), UNPINNED).unwrap();
    let local = lamp.root().join("var/lib/lampos-tasks/.local");
    fs::create_dir_all(&local).unwrap();
    symlink(&elsewhere, local.join("bin")).unwrap();
    let launcher = local.join("bin/harness");
    assert!(launcher::pin(&launcher, "/x").is_err());
    assert_eq!(
        fs::read_to_string(elsewhere.join("harness")).unwrap(),
        UNPINNED
    );
    assert!(!elsewhere.join(".harness.lampos-pin").exists());
}

/// A launcher with a second hard link (a name elsewhere for the same file):
/// left as it is, one line said, the other name unchanged (two security
/// reads' P3 on #5624).
#[test]
fn a_hard_linked_launcher_is_left_alone() {
    let lamp = Lamp::new();
    let launcher = launcher_left(&lamp, UNPINNED);
    let other = lamp.root().join("elsewhere");
    fs::hard_link(&launcher, &other).unwrap();
    let mut runs = Runs::new();
    runs.tasks_environment = HOLD_ON.into();
    let mut lines = Vec::new();
    let mut run = runs.runner();
    launcher::after_install(lamp.root(), &mut run, &mut |line: &str| {
        lines.push(line.to_owned())
    });
    assert_eq!(
        lines,
        [format!(
            "Harness's launcher not pinned: {}: hard-linked (2 names), left as it is",
            launcher.display()
        )]
    );
    assert_eq!(fs::read_to_string(&launcher).unwrap(), UNPINNED);
    assert_eq!(fs::read_to_string(&other).unwrap(), UNPINNED);
    assert!(!launcher.with_file_name(".harness.lampos-pin").exists());
}

/// A launcher over the 64 KiB cap (Harness's is a few hundred bytes): not
/// read or pinned, left byte for byte as it is, one line said (a security read's P3
/// on #5647).
#[test]
fn a_launcher_over_the_size_cap_is_left_alone() {
    let lamp = Lamp::new();
    let big = format!("#!/bin/sh\n{}{}", "#\n".repeat(64 * 1024), &UNPINNED[10..]);
    let launcher = launcher_left(&lamp, &big);
    let mut runs = Runs::new();
    runs.tasks_environment = HOLD_ON.into();
    let mut lines = Vec::new();
    let mut run = runs.runner();
    launcher::after_install(lamp.root(), &mut run, &mut |line: &str| {
        lines.push(line.to_owned())
    });
    assert_eq!(
        lines,
        [format!(
            "Harness's launcher not pinned: {}: {} bytes, over the 64 KiB cap, left as it is",
            launcher.display(),
            big.len()
        )]
    );
    assert_eq!(fs::read_to_string(&launcher).unwrap(), big);
    assert!(!launcher.with_file_name(".harness.lampos-pin").exists());
}

/// A launcher owned by another uid than its bin folder's owner is not
/// pinned; one with one name and the folder's owner is.
#[test]
fn a_launcher_owned_by_another_uid_is_not_pinned() {
    assert_eq!(launcher::refused(1, 1000, 1000), None);
    assert_eq!(
        launcher::refused(1, 0, 1000).as_deref(),
        Some("owned by uid 0, not the bin folder's owner 1000, left as it is")
    );
    assert_eq!(
        launcher::refused(3, 1000, 1000).as_deref(),
        Some("hard-linked (3 names), left as it is")
    );
}

/// The same through `pin`, where the test can give the launcher another
/// owner: only as root (skipped otherwise, e.g. on a builder's Mac).
#[test]
fn a_foreign_owned_launcher_is_left_alone_when_run_as_root() {
    let lamp = Lamp::new();
    if fs::metadata(lamp.root()).unwrap().uid() != 0 {
        return;
    }
    let launcher = launcher_left(&lamp, UNPINNED);
    std::os::unix::fs::chown(&launcher, Some(4242), None).unwrap();
    assert_eq!(
        launcher::pin(&launcher, "/x"),
        Err(format!(
            "{}: owned by uid 4242, not the bin folder's owner 0, left as it is",
            launcher.display()
        ))
    );
    assert_eq!(fs::read_to_string(&launcher).unwrap(), UNPINNED);
}

/// #5957: Harness recorded `installed` at a version older than the build's
/// pin, with nothing in use: the round asks the pinned one-shot to move it.
#[test]
fn an_older_installed_harness_is_asked_to_move_to_the_builds_pin() {
    let lamp = Lamp::new();
    lamp.harness_there();
    write(
        &lamp.records(),
        HarnessInstall {
            version: Some("0.4.25".into()),
            pinned: Some(true),
            ..state("installed", 1)
        },
    );
    let mut systemd = Fake::default();
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(Some(product::built_in())),
        &mut systemd,
        2,
        &mut quiet(),
    )
    .expect("an upgrade is asked");
    assert_eq!(said.install, "installing");
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
}

/// Harness recorded `installed` at `version` (#5957).
fn installed_at(lamp: &Lamp, version: Option<&str>, at: u64) {
    write(
        &lamp.records(),
        HarnessInstall {
            version: version.map(str::to_owned),
            pinned: Some(true),
            ..state("installed", at)
        },
    );
}

fn nothing_in_use() -> impl FnMut() -> Option<&'static str> {
    || None
}

/// #5957: versions are dotted numbers, never strings; an unreadable one is
/// never older.
#[test]
fn versions_are_compared_as_dotted_numbers() {
    for (have, want, said) in [
        ("0.5.9", "0.5.39", Some(true)),
        ("0.5.100", "0.5.39", Some(false)),
        ("0.5.39", "0.5.39", Some(false)),
        ("0.4.25", "0.5.39", Some(true)),
        ("1.0.0", "0.5.39", Some(false)),
        ("0.5", "0.5.1", Some(true)),
        ("0.5.0", "0.5", Some(false)),
        ("v0.5.1", "0.5.39", None),
        ("", "0.5.39", None),
        ("0.5.x", "0.5.39", None),
        ("0..1", "0.5.39", None),
        ("0.6.10", "0.6.2", Some(false)),
        ("0.5.98", "0.6.2", Some(true)),
        ("0.6.2", "0.5.98", Some(false)),
    ] {
        assert_eq!(older(have, want), said, "{have} vs {want}");
    }
}

/// #5957: an equal, newer or unreadable recorded version is left alone:
/// nothing asked, nothing started.
#[test]
fn an_equal_newer_or_unknown_version_is_left_alone() {
    // Newer than the pin, whatever the pin is: CI moves it by itself
    // (`.github/workflows/lampos-harness-pin.yml`, #5957 part 1), so a
    // fixed "newer" version would one day be older than the pin.
    let (major, rest) = product::VERSION.split_once('.').unwrap();
    let (minor, patch) = rest.split_once('.').unwrap();
    let patch_newer = format!("{major}.{minor}.{}", patch.parse::<u64>().unwrap() + 1);
    let major_newer = format!("{}.0.0", major.parse::<u64>().unwrap() + 1);
    for version in [
        Some(product::VERSION),
        Some(patch_newer.as_str()),
        Some(major_newer.as_str()),
        None,
    ] {
        let lamp = Lamp::new();
        lamp.harness_there();
        installed_at(&lamp, version, 1);
        let mut systemd = Fake::default();
        let said = after_round(
            lamp.root(),
            &lamp.records(),
            &Ok(Some(product::built_in())),
            &mut systemd,
            2,
            &mut quiet(),
        );
        assert_eq!(said, None, "{version:?}");
        assert!(systemd.asked.is_empty(), "{version:?}");
        assert!(!lamp.records().join(ASK).exists());
    }
    // Unpinned: nothing to move to.
    let lamp = Lamp::new();
    lamp.harness_there();
    installed_at(&lamp, Some("0.4.25"), 1);
    let mut systemd = Fake::default();
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        2,
        &mut quiet(),
    );
    assert_eq!(said, None);
    assert!(systemd.asked.is_empty());
}

/// #5957: the ask names the version moved from, and the record says it,
/// with the upgrade's own words; a round soon after starts no second.
#[test]
fn the_upgrade_ask_and_record_carry_the_version_moved_from() {
    let lamp = Lamp::new();
    lamp.harness_there();
    installed_at(&lamp, Some("0.4.25"), 1);
    let mut systemd = Fake::default();
    let pin = Ok(Some(product::built_in()));
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &pin,
        &mut systemd,
        2,
        &mut quiet(),
    )
    .unwrap();
    assert_eq!(
        said,
        HarnessInstall {
            revision: 2,
            ..HarnessInstall::upgrading("0.4.25", 2)
        }
    );
    assert!(said.check());
    let ask = asked(&lamp.records()).unwrap();
    assert_eq!(ask.upgrade_from.as_deref(), Some("0.4.25"));
    assert_eq!(ask.pin, Some(product::built_in()));
    let status = crate::status::status(&lamp.records(), "idle", None, None);
    assert_eq!(
        status.harness.unwrap().from_version.as_deref(),
        Some("0.4.25")
    );
    assert_eq!(
        after_round(
            lamp.root(),
            &lamp.records(),
            &pin,
            &mut systemd,
            3,
            &mut quiet()
        ),
        None,
        "a move under way is not asked twice"
    );
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
}

/// #5957: while a task is worked on, a sign-in is pending or a hand-over
/// is sent, a round says one line and changes nothing; the next idle round
/// asks the move.
#[test]
fn harness_in_use_defers_the_move_to_a_later_round() {
    let lamp = Lamp::new();
    lamp.harness_there();
    installed_at(&lamp, Some("0.4.25"), 1);
    let before = lamp.said();
    let mut systemd = Fake::default();
    let pin = Ok(Some(product::built_in()));
    for why in [
        "a handed-over task is being worked on",
        "a Harness sign-in is pending",
    ] {
        let mut lines = Vec::new();
        let said = after_round_when(
            lamp.root(),
            &lamp.records(),
            &pin,
            &mut systemd,
            2,
            &mut || Some(why),
            &mut |line: &str| lines.push(line.to_owned()),
        );
        assert_eq!(said, None);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].ends_with(why), "{lines:?}");
        assert!(lines[0].contains("at a later round"), "{lines:?}");
    }
    assert_eq!(lamp.said(), before, "the record is untouched");
    assert!(systemd.asked.is_empty());
    assert!(!lamp.records().join(ASK).exists());
    let said = after_round_when(
        lamp.root(),
        &lamp.records(),
        &pin,
        &mut systemd,
        3,
        &mut nothing_in_use(),
        &mut quiet(),
    );
    assert_eq!(said.unwrap().from_version.as_deref(), Some("0.4.25"));
    assert_eq!(systemd.asked, ["reload", "show", "start"]);
}

/// A runner for a move: `harness version` says `from` until the installer
/// ran, then `to`; the installer answers `installer`; `PUT_BACK` makes it
/// say `from` again.
fn mover(
    log: Rc<RefCell<Vec<String>>>,
    from: &'static str,
    to: &'static str,
    installer: Result<String, String>,
) -> impl FnMut(&[String]) -> Result<String, String> {
    let now = Rc::new(RefCell::new(from));
    move |argv: &[String]| {
        log.borrow_mut().push(argv.join(" "));
        if argv[0] == "id" {
            return Ok("990\n".into());
        }
        if install::runs_installer(argv) {
            *now.borrow_mut() = to;
            return installer.clone();
        }
        if argv.last().is_some_and(|last| last == PUT_BACK) {
            *now.borrow_mut() = from;
        }
        if argv.last().is_some_and(|last| last == "version") {
            return Ok(format!("harness {}\n", now.borrow()));
        }
        Ok(String::new())
    }
}

fn moved(lamp: &Lamp, run: &mut Runner, lines: &mut Vec<String>) -> HarnessInstall {
    install_now(
        lamp.root(),
        &lamp.records(),
        run,
        &mut || 5,
        &mut |line: &str| lines.push(line.to_owned()),
    )
    .unwrap()
}

fn asked_to_move(lamp: &Lamp, from: &str) {
    lamp.harness_there();
    installed_at(lamp, Some(from), 1);
    after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(Some(product::built_in())),
        &mut Fake::default(),
        2,
        &mut quiet(),
    )
    .unwrap();
}

/// #5957: the one-shot moves the Harness there to the pin, not "already
/// at": the daemon stopped, the Harness there kept as `lampos`, the pinned
/// installer from loopback, the version checked, the kept copy let go, and
/// lamp-tasks started again.
#[test]
fn the_one_shot_moves_an_older_harness_to_the_pin() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut run = mover(log.clone(), "0.4.25", product::VERSION, Ok(String::new()));
    let mut lines = Vec::new();
    let said = moved(&lamp, &mut run, &mut lines);
    assert_eq!(
        (
            said.install.as_str(),
            said.version.as_deref(),
            said.from_version.as_deref()
        ),
        ("installed", Some(product::VERSION), None),
        "{lines:?}"
    );
    assert!(said.check());
    let log = log.borrow().clone();
    let at = |part: &str| {
        log.iter()
            .position(|line| line.contains(part))
            .unwrap_or_else(|| panic!("{part}: {log:?}"))
    };
    let stop_tasks = at("systemctl stop lamp-tasks.service");
    let stop = at("/usr/local/bin/harness stop");
    assert!(stop_tasks < stop, "lamp-tasks is stopped first: {log:?}");
    let keep = log.iter().position(|line| line.ends_with(KEEP)).unwrap();
    let installer = log
        .iter()
        .position(|line| line.contains("HARNESS_METADATA_URL=http://127.0.0.1"))
        .unwrap();
    let let_go = log.iter().position(|line| line.ends_with(LET_GO)).unwrap();
    assert!(
        stop < keep && keep < installer && installer < let_go,
        "{log:?}"
    );
    assert!(log[keep].starts_with("runuser -u lampos -- env HOME=/var/lib/lampos-tasks "));
    assert!(!log.iter().any(|line| line.ends_with(PUT_BACK)), "{log:?}");
    assert_eq!(log.last().unwrap(), "systemctl restart lamp-tasks.service");
    assert!(
        lines.iter().any(|line| line
            == &format!(
                "Harness moved from 0.4.25 to the pinned {}",
                product::VERSION
            )),
        "{lines:?}"
    );
}

/// #5957: the one-shot finds Harness not older after all (or with no
/// version): nothing installed, `installed` as it is.
#[test]
fn the_one_shot_leaves_a_harness_that_is_not_older() {
    for (says, version) in [(product::VERSION, Some(product::VERSION)), ("none", None)] {
        let lamp = Lamp::new();
        asked_to_move(&lamp, "0.4.25");
        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut run = {
            let log = log.clone();
            move |argv: &[String]| -> Result<String, String> {
                log.borrow_mut().push(argv.join(" "));
                if argv[0] == "id" {
                    Ok("990\n".into())
                } else if argv.last().is_some_and(|last| last == "version") {
                    if says == "none" {
                        Err("exit status: 1".into())
                    } else {
                        Ok(format!("harness {says}\n"))
                    }
                } else {
                    Ok(String::new())
                }
            }
        };
        let said = moved(&lamp, &mut run, &mut Vec::new());
        assert_eq!(
            (said.install.as_str(), said.version.as_deref()),
            ("installed", version)
        );
        assert!(
            !log.borrow()
                .iter()
                .any(|line| line.contains("HARNESS_METADATA_URL"))
        );
        assert!(
            !log.borrow()
                .iter()
                .any(|line| line.ends_with("harness stop"))
        );
    }
}

/// #5957: a failed installer, or another version after it, puts the kept
/// Harness back and records `failed` with `upgrade` and `from_version`;
/// the next rounds with the same pin wait [`UPGRADE_RETRY_AFTER`], a new
/// pin's round tries at once.
#[test]
fn a_failed_move_puts_the_older_harness_back() {
    for (to, installer) in [
        ("0.4.25", Err("exit status: 1".to_owned())),
        ("0.5.40", Ok(String::new())),
    ] {
        let lamp = Lamp::new();
        asked_to_move(&lamp, "0.4.25");
        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut run = mover(log.clone(), "0.4.25", to, installer.clone());
        let mut lines = Vec::new();
        let said = moved(&lamp, &mut run, &mut lines);
        assert_eq!(
            said,
            HarnessInstall {
                revision: said.revision,
                ..HarnessInstall::upgrade_failed("0.4.25", 5)
            },
            "{installer:?}"
        );
        assert!(said.check());
        let log = log.borrow().clone();
        let back = log
            .iter()
            .position(|line| line.ends_with(PUT_BACK))
            .unwrap();
        assert!(
            log[back..].iter().any(|line| line
                == "ln -sfn /var/lib/lampos-tasks/.local/bin/harness /usr/local/bin/harness"),
            "Harness is linked again: {log:?}"
        );
        assert!(
            log.iter().any(|line| line.ends_with(LET_GO)),
            "put back, so let go"
        );
        assert!(
            lines.iter().any(|line| line == "Harness 0.4.25 put back"),
            "{lines:?}"
        );
        assert_eq!(log.last().unwrap(), "systemctl restart lamp-tasks.service");

        let mut systemd = Fake::default();
        let pin = Ok(Some(product::built_in()));
        let day = UPGRADE_RETRY_AFTER.as_millis() as u64;
        assert_eq!(
            after_round(
                lamp.root(),
                &lamp.records(),
                &pin,
                &mut systemd,
                5 + MINUTE_MS,
                &mut quiet()
            ),
            None,
            "the same pin waits"
        );
        assert_eq!(lamp.said().unwrap().error.as_deref(), Some("upgrade"));
        let again = after_round(
            lamp.root(),
            &lamp.records(),
            &pin,
            &mut systemd,
            5 + day,
            &mut quiet(),
        );
        assert_eq!(again.unwrap().from_version.as_deref(), Some("0.4.25"));
    }
    // Another pin (an env pin, its version read once fetched) at once.
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    write(&lamp.records(), HarnessInstall::upgrade_failed("0.4.25", 5));
    let other = Pin {
        url: "https://example.invalid/harness/manifest-0.6.0.json".into(),
        sha256: "ab".repeat(32),
        built_in: false,
    };
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(Some(other.clone())),
        &mut Fake::default(),
        6,
        &mut quiet(),
    );
    assert_eq!(said.unwrap().from_version.as_deref(), Some("0.4.25"));
    assert_eq!(asked(&lamp.records()).unwrap().pin, Some(other));
}

/// #5957: a copy that cannot be kept installs nothing: `upgrade`, Harness
/// as it was, lamp-tasks started again.
#[test]
fn a_move_whose_copy_cannot_be_kept_installs_nothing() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut inner = mover(log.clone(), "0.4.25", product::VERSION, Ok(String::new()));
    let mut run = |argv: &[String]| {
        if argv.last().is_some_and(|last| last == KEEP) {
            log.borrow_mut().push(argv.join(" "));
            return Err("cp: No space left on device".to_owned());
        }
        inner(argv)
    };
    let said = moved(&lamp, &mut run, &mut Vec::new());
    assert_eq!(said.error.as_deref(), Some("upgrade"));
    let log = log.borrow().clone();
    assert!(
        !log.iter().any(|line| line.contains("HARNESS_METADATA_URL")),
        "{log:?}"
    );
    assert!(!log.iter().any(|line| line.ends_with(PUT_BACK)));
    assert!(
        !log.iter().any(|line| line.ends_with(LET_GO)),
        "a copy already there is never let go: {log:?}"
    );
    assert_eq!(log.last().unwrap(), "systemctl restart lamp-tasks.service");
}

/// #5957: an env pin's version is read once fetched, so the round asks
/// when the pin differs from the last one handed over, and the one-shot
/// installs only a newer `cli.version`.
#[test]
fn an_env_pin_is_asked_when_it_changes_and_compared_by_the_one_shot() {
    let lamp = Lamp::new();
    lamp.harness_there();
    installed_at(&lamp, Some("0.4.25"), 1);
    let manifest = br#"{"cli":{"version":"0.4.20"}}"#;
    let pin = Pin {
        url: "https://example.invalid/harness/manifest.json".into(),
        sha256: crate::manifest::sha256_hex(manifest),
        built_in: false,
    };
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(Some(pin.clone())),
        &mut Fake::default(),
        2,
        &mut quiet(),
    );
    assert_eq!(said.unwrap().from_version.as_deref(), Some("0.4.25"));
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut inner = mover(log.clone(), "0.4.25", "0.4.20", Ok(String::new()));
    let mut run = |argv: &[String]| {
        if argv[0] == "curl" {
            return Ok(String::from_utf8(manifest.to_vec()).unwrap());
        }
        inner(argv)
    };
    let said = moved(&lamp, &mut run, &mut Vec::new());
    assert_eq!(
        (said.install.as_str(), said.version.as_deref()),
        ("installed", Some("0.4.25"))
    );
    assert!(
        !log.borrow()
            .iter()
            .any(|line| line.contains("HARNESS_METADATA_URL"))
    );
    // The same pin again: nothing asked.
    let mut systemd = Fake::default();
    assert_eq!(
        after_round(
            lamp.root(),
            &lamp.records(),
            &Ok(Some(pin)),
            &mut systemd,
            3,
            &mut quiet()
        ),
        None
    );
    assert!(systemd.asked.is_empty());
}

/// #5957: an ask written before `upgrade_from` reads as a first install's.
#[test]
fn an_ask_without_upgrade_from_still_reads() {
    let lamp = Lamp::new();
    fs::write(lamp.records().join(ASK), br#"{"version":1,"pin":null}"#).unwrap();
    let ask = asked(&lamp.records()).unwrap();
    assert_eq!((ask.pin, ask.upgrade_from), (None, None));
    let written = serde_json::to_string(&Ask {
        version: 1,
        pin: None,
        upgrade_from: None,
    })
    .unwrap();
    assert_eq!(written, r#"{"version":1,"pin":null}"#);
}

/// A launcher whose `cli.js` sits in Harness's home or `.harness` itself is
/// neither kept aside nor put back, so the sign-in in that folder is never
/// removed (#5957).
#[test]
fn a_cli_folder_that_holds_the_sign_in_is_never_kept_or_put_back() {
    for cli_dir in ["", "/.harness", "/.local/bin", "/./.harness", "//.harness"] {
        let home = tempfile::tempdir().unwrap();
        let h = home.path().to_str().unwrap().to_owned();
        let dir = format!("{h}{cli_dir}");
        std::fs::create_dir_all(format!("{h}/.local/bin")).unwrap();
        std::fs::create_dir_all(format!("{h}/.harness")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/cli.js"), "").unwrap();
        std::fs::write(format!("{h}/.harness/auth.json"), "signed in").unwrap();
        std::fs::write(
            format!("{h}/.local/bin/harness"),
            format!("#!/bin/sh\nexec node '{dir}/cli.js' \"$@\"\n"),
        )
        .unwrap();
        let run = |script: &str| {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(script)
                .env("HOME", &h)
                .status()
                .unwrap()
        };
        assert!(!run(KEEP).success(), "kept {cli_dir:?}");
        std::fs::create_dir_all(format!("{h}/.harness/lampos-upgrade-from/cli")).unwrap();
        std::fs::write(format!("{h}/.harness/lampos-upgrade-from/harness"), "").unwrap();
        std::fs::write(
            format!("{h}/.harness/lampos-upgrade-from/cli-dir"),
            format!("{dir}\n"),
        )
        .unwrap();
        assert!(!run(PUT_BACK).success(), "put back {cli_dir:?}");
        assert_eq!(
            std::fs::read_to_string(format!("{h}/.harness/auth.json")).unwrap(),
            "signed in"
        );
    }
}

/// `script` run by `sh` with `home` as its `HOME`, as the one-shot runs it
/// as `lampos`.
fn sh_in(script: &str, home: &str) -> std::process::ExitStatus {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .env("HOME", home)
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
}

/// A Harness laid out as its installer leaves it under `home`: the CLI
/// folder (with `file`), its launcher, a runtime pointer and the tmux link.
fn harness_home(file: &str) -> (tempfile::TempDir, String) {
    let home = tempfile::tempdir().unwrap();
    let h = home.path().to_str().unwrap().to_owned();
    for dir in [".harness/cli", ".harness/runtime/node-1/bin", ".local/bin"] {
        fs::create_dir_all(format!("{h}/{dir}")).unwrap();
    }
    fs::write(format!("{h}/.harness/cli/{file}"), file).unwrap();
    fs::write(format!("{h}/.harness/cli/cli.js"), "").unwrap();
    fs::write(
        format!("{h}/.local/bin/harness"),
        format!("#!/bin/sh\nexec '{h}/.harness/runtime/node-1/bin/node' '{h}/.harness/cli/cli.js' \"$@\"\n"),
    )
    .unwrap();
    fs::write(format!("{h}/.harness/runtime/current-node"), "node-1\n").unwrap();
    fs::write(format!("{h}/.harness/auth.json"), "signed in").unwrap();
    symlink(
        format!("{h}/.harness/runtime/node-1"),
        format!("{h}/.local/bin/tmux"),
    )
    .unwrap();
    (home, h)
}

/// #5957: what KEEP kept, PUT_BACK puts back in a real shell: the CLI
/// folder, the launcher, the runtime pointer and the tmux link, with the
/// sign-in untouched.
#[test]
fn keep_then_put_back_restores_the_older_harness() {
    let (_home, h) = harness_home("old.js");
    assert!(sh_in(KEEP, &h).success());
    // The installer's move: a new CLI, launcher, pointer and link.
    fs::remove_file(format!("{h}/.harness/cli/old.js")).unwrap();
    fs::write(format!("{h}/.harness/cli/new.js"), "").unwrap();
    fs::write(format!("{h}/.local/bin/harness"), "#!/bin/sh\nexec new\n").unwrap();
    fs::write(format!("{h}/.harness/runtime/current-node"), "node-2\n").unwrap();
    fs::create_dir_all(format!("{h}/.harness/runtime/node-2")).unwrap();
    fs::remove_file(format!("{h}/.local/bin/tmux")).unwrap();
    symlink(
        format!("{h}/.harness/runtime/node-2"),
        format!("{h}/.local/bin/tmux"),
    )
    .unwrap();
    assert!(sh_in(PUT_BACK, &h).success());
    assert!(Path::new(&format!("{h}/.harness/cli/old.js")).exists());
    assert!(!Path::new(&format!("{h}/.harness/cli/new.js")).exists());
    assert!(
        fs::read_to_string(format!("{h}/.local/bin/harness"))
            .unwrap()
            .contains("/.harness/cli/cli.js")
    );
    assert_eq!(
        fs::read_to_string(format!("{h}/.harness/runtime/current-node")).unwrap(),
        "node-1\n"
    );
    assert_eq!(
        fs::read_link(format!("{h}/.local/bin/tmux")).unwrap(),
        PathBuf::from(format!("{h}/.harness/runtime/node-1"))
    );
    assert!(
        fs::symlink_metadata(format!("{h}/.harness/runtime/node-2/node-1")).is_err(),
        "the link was replaced, not followed"
    );
    assert_eq!(
        fs::read_to_string(format!("{h}/.harness/auth.json")).unwrap(),
        "signed in"
    );
}

/// The Node the pinned runtime manifest names for a lamp (linux-arm64).
fn pinned_node() -> String {
    let manifest: serde_json::Value = serde_json::from_slice(product::MANIFESTS[0].bytes).unwrap();
    manifest["node"]["linux-arm64"]["version"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A runner for a move on a real Harness home `h`: KEEP, PUT_BACK and
/// LET_GO run in a real shell; the installer does what Harness's does with
/// Node (#6302): it reuses the Node `current-node` names, else takes the
/// one the pinned runtime manifest names (`node_step`, its result).
fn node_mover(
    log: Rc<RefCell<Vec<String>>>,
    h: String,
    node_step: Result<String, String>,
) -> impl FnMut(&[String]) -> Result<String, String> {
    let now = Rc::new(RefCell::new("0.4.25"));
    move |argv: &[String]| {
        log.borrow_mut().push(argv.join(" "));
        if argv[0] == "id" {
            return Ok("990\n".into());
        }
        let last = argv.last().unwrap();
        if [KEEP, PUT_BACK, LET_GO].contains(&last.as_str()) {
            if !sh_in(last, &h).success() {
                return Err("exit status: 1".into());
            }
            if last == PUT_BACK {
                *now.borrow_mut() = "0.4.25";
            }
            return Ok(String::new());
        }
        if install::runs_installer(argv) {
            let current = format!("{h}/.harness/runtime/current-node");
            if fs::metadata(&current).is_err() {
                node_step.clone()?;
                let node = pinned_node();
                fs::write(
                    &current,
                    format!("{h}/.harness/runtime/node-{node}-linux-arm64/bin/node\n"),
                )
                .unwrap();
            }
            *now.borrow_mut() = product::VERSION;
            return Ok(String::new());
        }
        if last == "version" {
            return Ok(format!("harness {}\n", now.borrow()));
        }
        Ok(String::new())
    }
}

/// #6302: a move from an older Harness whose runtime names an older Node
/// ends on the Node the pinned runtime manifest names, served from
/// loopback (checked by its sha256), not on the older one, and `installed`.
#[test]
fn a_move_ends_on_the_pinned_node() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    let (_home, h) = harness_home("old.js");
    let old = format!("{h}/.harness/runtime/node-v22.0.0-linux-arm64/bin/node\n");
    fs::write(format!("{h}/.harness/runtime/current-node"), &old).unwrap();
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut run = node_mover(log.clone(), h.clone(), Ok(String::new()));
    let mut lines = Vec::new();
    let said = moved(&lamp, &mut run, &mut lines);
    assert_eq!(
        (said.install.as_str(), said.version.as_deref()),
        ("installed", Some(product::VERSION)),
        "{lines:?}"
    );
    assert_eq!(
        fs::read_to_string(format!("{h}/.harness/runtime/current-node")).unwrap(),
        format!(
            "{h}/.harness/runtime/node-{}-linux-arm64/bin/node\n",
            pinned_node()
        )
    );
    assert!(
        log.borrow()
            .iter()
            .any(|line| install::runs_installer(
                &line.split(' ').map(str::to_owned).collect::<Vec<_>>()
            ) && line.contains("HARNESS_RUNTIME_METADATA_URL=http://127.0.0.1")),
        "the pinned runtime manifest is handed over: {:?}",
        log.borrow()
    );
}

/// #6302: the pinned Node not taken (its download or sha256 failing in the
/// installer) records `failed` with `upgrade`, and the older Harness and
/// its `current-node` are put back.
#[test]
fn a_failed_node_step_puts_the_older_node_back() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    let (_home, h) = harness_home("old.js");
    let old = format!("{h}/.harness/runtime/node-v22.0.0-linux-arm64/bin/node\n");
    fs::write(format!("{h}/.harness/runtime/current-node"), &old).unwrap();
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut run = node_mover(
        log.clone(),
        h.clone(),
        Err("Node download failed checksum verification".into()),
    );
    let mut lines = Vec::new();
    let said = moved(&lamp, &mut run, &mut lines);
    assert_eq!(
        said,
        HarnessInstall {
            revision: said.revision,
            ..HarnessInstall::upgrade_failed("0.4.25", 5)
        },
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("checksum verification")),
        "the reason is said: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line == "Harness 0.4.25 put back"),
        "{lines:?}"
    );
    assert_eq!(
        fs::read_to_string(format!("{h}/.harness/runtime/current-node")).unwrap(),
        old
    );
    assert!(Path::new(&format!("{h}/.harness/cli/old.js")).exists());
    assert!(
        !Path::new(&format!("{h}/{KEPT}")).exists(),
        "let go once put back"
    );
}

/// #5957 (P2-1): a kept copy without its CLI folder stops PUT_BACK before
/// it removes anything: `set -e` does not stop at a failed `a` in `a && b`.
#[test]
fn put_back_refuses_a_kept_copy_without_its_cli_folder() {
    let (_home, h) = harness_home("new.js");
    let k = format!("{h}/{KEPT}");
    fs::create_dir_all(&k).unwrap();
    fs::write(format!("{k}/harness"), "").unwrap();
    fs::write(format!("{k}/cli-dir"), format!("{h}/.harness/cli\n")).unwrap();
    assert!(!sh_in(PUT_BACK, &h).success());
    assert!(
        Path::new(&format!("{h}/.harness/cli/new.js")).exists(),
        "the CLI folder there is kept"
    );
}

/// #5957 (P2-3): a kept copy already there (a put-back that failed) is the
/// only good one: KEEP refuses, and leaves it whole.
#[test]
fn keep_refuses_when_a_kept_copy_is_already_there() {
    let (_home, h) = harness_home("old.js");
    let k = format!("{h}/{KEPT}");
    fs::create_dir_all(&k).unwrap();
    fs::write(format!("{k}/keep-me"), "").unwrap();
    assert!(!sh_in(KEEP, &h).success());
    assert!(Path::new(&format!("{k}/keep-me")).exists());
}

/// #5957 (pre-review P2-4a): Harness in use when the one-shot looks again,
/// just before it stops it: nothing stopped or installed, `installed` as
/// it was, and the next round asks again.
#[test]
fn the_one_shot_waits_while_harness_is_in_use() {
    let kit = Kit {
        in_use: |_| Some("a Harness sign-in is pending"),
        ..Kit::BUILT
    };
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut run = mover(log.clone(), "0.4.25", product::VERSION, Ok(String::new()));
    let mut lines = Vec::new();
    let said = install_with(
        lamp.root(),
        &lamp.records(),
        &mut run,
        &mut || 5,
        &mut |line: &str| lines.push(line.to_owned()),
        &kit,
    )
    .unwrap();
    assert_eq!(
        (
            said.install.as_str(),
            said.version.as_deref(),
            said.from_version.as_deref()
        ),
        ("installed", Some("0.4.25"), None)
    );
    let log = log.borrow().clone();
    for never in ["stop", "HARNESS_METADATA_URL", "restart lamp-tasks"] {
        assert!(
            !log.iter().any(|line| line.contains(never)),
            "{never}: {log:?}"
        );
    }
    assert!(!log.iter().any(|line| line.ends_with(KEEP)));
    assert!(
        lines
            .iter()
            .any(|line| line.ends_with("a Harness sign-in is pending")),
        "{lines:?}"
    );
    let mut systemd = Fake::default();
    let again = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(Some(product::built_in())),
        &mut systemd,
        6,
        &mut quiet(),
    );
    assert_eq!(again.unwrap().from_version.as_deref(), Some("0.4.25"));
}

/// #5957 (pre-review P2-4b/c): lamp-tasks or Harness's daemon not stopped
/// is "not now": nothing kept or installed, `installed` as it was, and
/// lamp-tasks started again.
#[test]
fn a_harness_not_stopped_is_moved_at_a_later_round() {
    for failing in [
        "systemctl stop lamp-tasks.service",
        "/usr/local/bin/harness stop",
    ] {
        let lamp = Lamp::new();
        asked_to_move(&lamp, "0.4.25");
        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut inner = mover(log.clone(), "0.4.25", product::VERSION, Ok(String::new()));
        let mut run = |argv: &[String]| {
            if argv.join(" ").ends_with(failing) {
                log.borrow_mut().push(argv.join(" "));
                return Err("exit status: 1".to_owned());
            }
            inner(argv)
        };
        let said = moved(&lamp, &mut run, &mut Vec::new());
        assert_eq!(
            (said.install.as_str(), said.version.as_deref()),
            ("installed", Some("0.4.25")),
            "{failing}"
        );
        let log = log.borrow().clone();
        assert!(!log.iter().any(|line| line.ends_with(KEEP)), "{log:?}");
        assert!(!log.iter().any(|line| line.contains("HARNESS_METADATA_URL")));
        assert_eq!(log.last().unwrap(), "systemctl start lamp-tasks.service");
        if failing.starts_with("systemctl") {
            assert!(
                !log.iter().any(|line| line.ends_with("harness stop")),
                "{log:?}"
            );
        }
    }
}

/// #5957 (a lamp row's P3): Harness had moved itself past the recorded
/// version; the one-shot's `installing` and `failed` `upgrade` records name
/// the version Harness says, not the record's.
#[test]
fn the_move_names_the_version_harness_says_not_the_record() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    let records = lamp.records();
    let seen: Rc<RefCell<Option<HarnessInstall>>> = Rc::default();
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut inner = mover(
        log.clone(),
        "0.4.30",
        "0.4.30",
        Err("exit status: 1".to_owned()),
    );
    let mut run = |argv: &[String]| {
        if install::runs_installer(argv) {
            *seen.borrow_mut() = record(&records);
        }
        inner(argv)
    };
    let said = moved(&lamp, &mut run, &mut Vec::new());
    let during = seen.borrow().clone().unwrap();
    assert_eq!(
        (during.install.as_str(), during.from_version.as_deref()),
        ("installing", Some("0.4.30"))
    );
    assert!(during.check());
    assert_eq!(
        (said.error.as_deref(), said.from_version.as_deref()),
        (Some("upgrade"), Some("0.4.30"))
    );
}

/// #5957 (a re-read's P3): `id -u` failing at the stop leaves lamp-tasks
/// never stopped: nothing stopped or kept, and lamp-tasks only started
/// (`start`, a running one left as it is), never restarted.
#[test]
fn a_user_id_not_read_at_the_stop_never_stops_lamp_tasks() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut inner = mover(log.clone(), "0.4.25", product::VERSION, Ok(String::new()));
    let ids = RefCell::new(0);
    let mut run = |argv: &[String]| {
        if argv[0] == "id" {
            *ids.borrow_mut() += 1;
            if *ids.borrow() > 1 {
                log.borrow_mut().push(argv.join(" "));
                return Err("id: no such user".to_owned());
            }
        }
        inner(argv)
    };
    let said = moved(&lamp, &mut run, &mut Vec::new());
    assert_eq!(
        (said.install.as_str(), said.version.as_deref()),
        ("installed", Some("0.4.25"))
    );
    let log = log.borrow().clone();
    assert!(!log.iter().any(|line| line.contains(" stop")), "{log:?}");
    assert!(!log.iter().any(|line| line.contains("restart")), "{log:?}");
    assert!(!log.iter().any(|line| line.ends_with(KEEP)));
    assert_eq!(log.last().unwrap(), "systemctl start lamp-tasks.service");
}

/// #5957 (a re-read's P3): a kept copy already there is looked at before
/// anything is stopped: nothing stopped, kept, installed or restarted, a
/// `failed` `upgrade`, and the copy left whole.
#[test]
fn a_kept_copy_already_there_stops_nothing() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.4.25");
    let kept = lamp.root().join("var/lib/lampos-tasks").join(KEPT);
    fs::create_dir_all(&kept).unwrap();
    fs::write(kept.join("keep-me"), "").unwrap();
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut run = mover(log.clone(), "0.4.25", product::VERSION, Ok(String::new()));
    let mut lines = Vec::new();
    let said = moved(&lamp, &mut run, &mut lines);
    assert_eq!(
        (said.error.as_deref(), said.from_version.as_deref()),
        (Some("upgrade"), Some("0.4.25"))
    );
    let log = log.borrow().clone();
    for never in ["systemctl", " stop", "HARNESS_METADATA_URL"] {
        assert!(
            !log.iter().any(|line| line.contains(never)),
            "{never}: {log:?}"
        );
    }
    assert!(
        !log.iter()
            .any(|line| line.ends_with(KEEP) || line.ends_with(LET_GO))
    );
    assert!(kept.join("keep-me").exists());
    assert!(
        lines.iter().any(|line| line
            == "a kept copy waits at /var/lib/lampos-tasks/.harness/lampos-upgrade-from; Harness is not moved until it is cleared"),
        "{lines:?}"
    );
}

/// The one-shot (Harness's install, then Hermes's) yields to the lamp's own
/// services, as lamp-update does: on lamp-c0a4 on 10 Nov a Hermes install at
/// the default priority stalled lamp-controls' privacy reading (#5978). Its
/// memory is held under `MemoryHigh` (throttled past it, never killed, as
/// no `MemoryMax` is set), so a 4 GB lamp keeps about 1.2 GB available: the
/// capped install's `products` stage took `MemAvailable` to about 868 MB.
#[test]
fn the_one_shot_runs_below_the_lamps_services() {
    let lines: Vec<&str> = TEXT.lines().collect();
    for wanted in [
        "Nice=10",
        "IOSchedulingClass=idle",
        "CPUWeight=20",
        "CPUQuota=100%",
        "IOWriteBandwidthMax=/var/lib 8M",
        "IOReadBandwidthMax=/var/lib 16M",
        "IOWriteIOPSMax=/var/lib 200",
        "IOReadIOPSMax=/var/lib 400",
        "MemoryHigh=600M",
    ] {
        assert!(
            lines.contains(&wanted),
            "{wanted} missing from the one-shot"
        );
    }
    // Throttled, never killed: no hard cap, no swap cap.
    for unwanted in ["MemoryMax=", "MemorySwapMax=", "MemoryLimit="] {
        assert!(
            !lines.iter().any(|line| line.starts_with(unwanted)),
            "{unwanted} in the one-shot"
        );
    }
}

/// The one-shot as its unit runs it: Harness's part, then Hermes's.
fn one_shot(lamp: &Lamp, run: &mut Runner, lines: &mut Vec<String>) -> Result<(), String> {
    crate::hermes_install::one_shot(
        lamp.root(),
        &lamp.records(),
        run,
        &mut || 5,
        &mut |line: &str| lines.push(line.to_owned()),
    )
}

/// Whether the pinned installer ran, as `log` has it.
fn installer_ran(log: &Rc<RefCell<Vec<String>>>) -> bool {
    log.borrow()
        .iter()
        .any(|line| line.contains("HARNESS_METADATA_URL=http://127.0.0.1"))
}

/// #6275: a round asks to move Harness 0.5.39 to the build's pin; the
/// one-shot runs the move, not Hermes's part alone, so the record ends
/// `installed` at the pin or `failed` with `upgrade`, never `installing`.
#[test]
fn the_one_shot_runs_a_move_a_round_asked() {
    for (installer, install, version, error) in [
        (Ok(String::new()), "installed", Some(product::VERSION), None),
        (
            Err("sh: exit status 1".to_owned()),
            "failed",
            None,
            Some("upgrade"),
        ),
    ] {
        let lamp = Lamp::new();
        asked_to_move(&lamp, "0.5.39");
        assert_eq!(lamp.said().unwrap().install, "installing");
        let log: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut run = mover(log.clone(), "0.5.39", product::VERSION, installer);
        let mut lines = Vec::new();
        let _ = one_shot(&lamp, &mut run, &mut lines);
        let said = lamp.said().unwrap();
        assert_eq!(
            (
                said.install.as_str(),
                said.version.as_deref(),
                said.error.as_deref()
            ),
            (install, version, error),
            "{lines:?}"
        );
        assert!(installer_ran(&log), "{:?}", log.borrow());
    }
}

/// #6275: Harness there, an install's ask (no `upgrade_from`) and its
/// record left `installing`: the one-shot records it `installed` with the
/// version it says.
#[test]
fn the_one_shot_answers_an_install_ask_with_harness_there() {
    let lamp = Lamp::new();
    let said = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(Some(product::built_in())),
        &mut Fake::default(),
        2,
        &mut quiet(),
    )
    .unwrap();
    assert_eq!(said.install, "installing");
    lamp.harness_there();
    let runs = Runs::new();
    let mut run = runs.runner();
    one_shot(&lamp, &mut run, &mut Vec::new()).unwrap();
    drop(run);
    let said = lamp.said().unwrap();
    assert_eq!(
        (said.install.as_str(), said.version.as_deref()),
        ("installed", Some("0.4.25"))
    );
    assert!(!runs.ran(install::HARNESS_INSTALLER));
}

/// #6275: started for Hermes alone, with no Harness ask, Harness's record
/// is left as it is and nothing of Harness's runs.
#[test]
fn the_one_shot_without_a_harness_ask_leaves_harness_as_it_is() {
    let lamp = Lamp::new();
    lamp.harness_there();
    installed_at(&lamp, Some("0.5.39"), 1);
    let before = lamp.said().unwrap();
    let runs = Runs::new();
    let mut run = runs.runner();
    one_shot(&lamp, &mut run, &mut Vec::new()).unwrap();
    drop(run);
    assert_eq!(lamp.said().unwrap(), before);
    assert!(runs.log.borrow().is_empty(), "{:?}", runs.log.borrow());
}

/// #6275: a move answered spends its ask: a failed move, then a one-shot
/// started for Hermes, moves nothing again and keeps the `failed`
/// `upgrade`, so the round's [`UPGRADE_RETRY_AFTER`] paces the next try.
#[test]
fn a_failed_move_is_not_run_again_by_a_one_shot_for_hermes() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.5.39");
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut run = mover(
        log.clone(),
        "0.5.39",
        product::VERSION,
        Err("sh: exit status 1".to_owned()),
    );
    let _ = one_shot(&lamp, &mut run, &mut Vec::new());
    let failed = lamp.said().unwrap();
    assert_eq!(
        (failed.install.as_str(), failed.error.as_deref()),
        ("failed", Some("upgrade"))
    );
    let ask = asked(&lamp.records()).unwrap();
    assert_eq!(
        (ask.upgrade_from, ask.pin),
        (None, Some(product::built_in())),
        "spent, its pin kept"
    );

    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut run = mover(log.clone(), "0.5.39", product::VERSION, Ok(String::new()));
    let mut lines = Vec::new();
    // The unit's result is Harness's record, kept `failed`.
    assert_eq!(
        one_shot(&lamp, &mut run, &mut lines),
        Err("Harness is not installed: upgrade".to_owned())
    );
    assert!(!installer_ran(&log), "moved again: {:?}", log.borrow());
    assert_eq!(lamp.said().unwrap(), failed, "{lines:?}");
}

/// #6275: the one exit that leaves a move `installing`: another install
/// holds [`LOCK`] and writes its own record; the unit fails, and a round
/// past [`STUCK_AFTER`] asks the move again.
#[test]
fn a_move_with_the_lock_held_stays_installing_for_its_holder() {
    let lamp = Lamp::new();
    asked_to_move(&lamp, "0.5.39");
    let _held = lock(&lamp.records()).unwrap();
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut run = mover(log.clone(), "0.5.39", product::VERSION, Ok(String::new()));
    assert!(one_shot(&lamp, &mut run, &mut Vec::new()).is_err());
    assert!(log.borrow().is_empty());
    assert_eq!(lamp.said().unwrap().install, "installing");
}
