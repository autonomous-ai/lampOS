use super::*;
use crate::judge::UnitState;
use std::{cell::RefCell, os::unix::fs::symlink, path::PathBuf, rc::Rc};

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
    fn harness_there(&self) {
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
}

impl Runs {
    fn new() -> Self {
        Self {
            log: Rc::default(),
            installer: Ok(String::new()),
            manifest: Ok(String::new()),
            id: Ok("990\n".into()),
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
            } else if line.contains(install::HARNESS_INSTALLER) {
                self.installer.clone()
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
        crate::status::status(&lamp.records(), "idle", None).harness,
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
        crate::status::status(&lamp.records(), "idle", None)
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
        let read = crate::status::status(&lamp.records(), "idle", None);
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
    let status = crate::status::status(&lamp.records(), "idle", None);
    assert_eq!(status.harness.unwrap().error.as_deref(), Some("installer"));

    // The retry: the next round starts the one-shot again.
    let retried = after_round(
        lamp.root(),
        &lamp.records(),
        &Ok(None),
        &mut systemd,
        3,
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
        .find(|line| line.contains(install::HARNESS_INSTALLER))
        .unwrap();
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
        if line.contains(install::HARNESS_INSTALLER) {
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
