use lamp_bus::Bus;
use lamp_msgs::{ClockStamp, TouchEvent, TouchGesture, TouchIntent, TouchSource};
use lamp_setup::{
    Watched,
    reset::{self, Effects, Plan, Step, accept},
};
use std::{
    cell::RefCell,
    fs, io,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const CLOCK: &str = "setup-test-boot";
const PROGRAM: &str = env!("CARGO_BIN_EXE_lamp-setup");
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "lamp-setup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::set_permissions(
            self.0.join("memory/locked"),
            fs::Permissions::from_mode(0o700),
        );
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A lamp's worth of personal data under one root, and a file outside it
/// that a link inside points at.
fn lamp(root: &Root) -> Plan {
    for dir in ["memory/days", "tasks", "setup", "outside"] {
        fs::create_dir_all(root.path(dir)).unwrap();
    }
    fs::write(root.path("memory/days/2026-10-15.json"), b"{}").unwrap();
    fs::write(root.path("memory/index.json"), b"{}").unwrap();
    fs::write(root.path("tasks/1.json"), b"{}").unwrap();
    fs::write(root.path("people.json"), b"[]").unwrap();
    fs::write(
        root.path("people.json.new"),
        b"synthetic unfinished ordinary save",
    )
    .unwrap();
    fs::write(
        root.path("people.json.repair.new"),
        b"synthetic unfinished repair",
    )
    .unwrap();
    fs::write(root.path("outside/keep.txt"), b"keep").unwrap();
    symlink(root.path("outside"), root.path("memory/link-to-outside")).unwrap();
    symlink(root.path("outside/keep.txt"), root.path("thread.json")).unwrap();
    fs::write(root.path("setup/done"), b"").unwrap();
    Plan {
        marker: root.path("setup/done"),
        journal: root.path("setup/reset.json"),
        wipe: vec![
            root.path("memory"),
            root.path("tasks"),
            root.path("people.json"),
            root.path("people.json.new"),
            root.path("people.json.repair.new"),
            root.path("thread.json"),
            root.path("schedule"),
        ],
        stop: vec![
            "/bin/systemctl".into(),
            "stop".into(),
            "lampos.target".into(),
        ],
        reboot: vec!["/bin/systemctl".into(), "reboot".into()],
        detach: vec![],
    }
}

/// The lamp's effects. `parts`: what `systemctl show -p ConsistsOf` prints
/// of `lampos.target` (`parts_after` from its second read on), and
/// `lamp-sub.target` is made of `lamp-sub-a.service`; `states`: what
/// `ActiveState` prints, each part `inactive` without it; `fail_read`:
/// nothing can be read. `writes_on_stop`: a part, `lamp-voice.service`,
/// writes this file as it stops, as systemd 252 runs it: before a stop that
/// names it returns, else after it (here, at the next command, the reboot).
/// `starters` and `writers`: what `systemctl list-units` lists of
/// `reset::STARTERS` and of `reset::WRITERS` (`writers_after` from the
/// second listing on). `writer`: the unit that writes `writes_on_stop`,
/// `lamp-voice.service` without it.
#[derive(Default)]
struct Fake {
    calls: Rc<RefCell<Vec<String>>>,
    argvs: Rc<RefCell<Vec<Vec<String>>>>,
    parts: String,
    parts_after: Option<String>,
    lampos_reads: usize,
    starters: String,
    writers: String,
    writers_after: Option<String>,
    writer_reads: usize,
    writer: Option<String>,
    /// The updater's way back with a self-switch pending: once
    /// `lamp-update.service` is stopped by a stop that comes before any stop
    /// of `lamp-update-back.service`, it restarts the updater, too late for
    /// the final check, and the updater writes this file (at the next
    /// command, the reboot).
    back_worker: Option<PathBuf>,
    back_stopped: bool,
    states: Option<String>,
    fail_read: bool,
    fail_stop: bool,
    fail_reboot: bool,
    /// `systemctl start`, the start in place after a product reset (#4500), fails.
    fail_start: bool,
    /// Emptying [`reset::VOLATILE`] before the start in place fails.
    fail_clear: bool,
    /// Every [`reset::FORGET`] command fails (no `wpa_cli` or `journalctl`).
    fail_forget: bool,
    writes_on_stop: Option<PathBuf>,
    pending: Option<PathBuf>,
}
impl Effects for Fake {
    fn run(&mut self, argv: &[String]) -> io::Result<()> {
        if let Some(late) = self.pending.take() {
            fs::write(late, b"{\"steps\":\"the factory's\"}").unwrap();
        }
        self.calls.borrow_mut().push(argv[1].clone());
        self.argvs.borrow_mut().push(argv.to_vec());
        if (self.fail_stop && argv[1] == "stop")
            || (self.fail_reboot && argv[1] == "reboot")
            || (self.fail_start && argv[1] == "start")
            || (self.fail_forget
                && (argv[0] == "/usr/sbin/wpa_cli" || argv[0] == "/usr/bin/journalctl"))
        {
            return Err(io::Error::other("refused"));
        }
        if argv[1] == "stop" {
            let names = |unit: &str| argv.iter().any(|named| named == unit);
            if !self.back_stopped
                && names("lamp-update.service")
                && let Some(file) = self.back_worker.take()
            {
                self.pending = Some(file);
            }
            self.back_stopped |= names("lamp-update-back.service");
        }
        if argv[1] == "stop"
            && let Some(file) = self.writes_on_stop.take()
        {
            let writer = self.writer.as_deref().unwrap_or("lamp-voice.service");
            if argv.iter().any(|unit| unit == writer) {
                fs::write(file, b"{\"steps\":\"the factory's\"}").unwrap();
            } else {
                self.pending = Some(file);
            }
        }
        Ok(())
    }
    fn read(&mut self, argv: &[String]) -> io::Result<String> {
        self.calls.borrow_mut().push(argv[1].clone());
        self.argvs.borrow_mut().push(argv.to_vec());
        if self.fail_read {
            return Err(io::Error::other("refused"));
        }
        if argv[1] == "list-units" {
            let listed = if argv.iter().any(|unit| unit == "lamp-skills-manage.socket") {
                self.starters.clone()
            } else {
                self.writer_reads += 1;
                match &self.writers_after {
                    Some(after) if self.writer_reads > 1 => after.clone(),
                    _ => self.writers.clone(),
                }
            };
            // As systemd lists them: only units the names and patterns asked
            // for match, so a unit left out of a list is never listed.
            return Ok(listed
                .lines()
                .filter(|line| {
                    let unit = line.split_whitespace().find(|word| *word != "\u{25cf}");
                    unit.is_some_and(|unit| argv[6..].iter().any(|asked| glob(asked, unit)))
                })
                .map(|line| format!("{line}\n"))
                .collect());
        }
        if argv[3] == "ActiveState" {
            return Ok(self
                .states
                .clone()
                .unwrap_or_else(|| argv[5..].iter().map(|_| "inactive\n\n").collect()));
        }
        Ok(match argv.last().map(String::as_str) {
            Some("lampos.target") => {
                self.lampos_reads += 1;
                match &self.parts_after {
                    Some(after) if self.lampos_reads > 1 => after.clone(),
                    _ => self.parts.clone(),
                }
            }
            Some("lamp-sub.target") => "lamp-sub-a.service\n".into(),
            _ => String::new(),
        })
    }
    fn sync(&mut self) {
        self.calls.borrow_mut().push("sync".into());
    }
    fn force_reboot(&mut self) {
        self.calls.borrow_mut().push("force".into());
    }
    fn clear_volatile(&mut self) -> io::Result<()> {
        self.calls.borrow_mut().push("clear".into());
        if self.fail_clear {
            return Err(io::Error::other("busy"));
        }
        Ok(())
    }
}

/// A unit name against a `systemctl` pattern, `*` standing for any run of
/// characters.
fn glob(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        let Some(at) = rest.find(part) else {
            return false;
        };
        rest = &rest[at + part.len()..];
    }
    rest.ends_with(last)
}

fn root_user() -> bool {
    rustix::process::geteuid().is_root()
}

fn wiped(root: &Root) {
    assert!(
        root.path("memory").is_dir(),
        "a wiped directory keeps itself"
    );
    assert_eq!(fs::read_dir(root.path("memory")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(root.path("tasks")).unwrap().count(), 0);
    assert!(!root.path("people.json").exists());
    assert!(!root.path("people.json.new").exists());
    assert!(!root.path("people.json.repair.new").exists());
    assert!(fs::symlink_metadata(root.path("thread.json")).is_err());
    assert_eq!(fs::read(root.path("outside/keep.txt")).unwrap(), b"keep");
    assert!(!root.path("setup/done").exists());
    assert!(!root.path("setup/reset.json").exists());
}

#[test]
fn a_reset_stops_wipes_removes_the_marker_and_reboots_in_that_order() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake::default();
    let steps = reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    assert_eq!(
        &steps[..3],
        [Step::Journal, Step::MarkerRemoved, Step::Stopped]
    );
    assert_eq!(
        &steps[steps.len() - 2..],
        [Step::JournalRemoved, Step::Rebooting]
    );
    assert_eq!(
        *fake.calls.borrow(),
        [
            "sync",
            "list-units",
            "show",
            "list-units",
            "stop",
            "sync",
            "sync",
            "reboot"
        ]
    );
}

/// lamp-29e7 on 26 Oct: `systemctl stop lampos.target` returned before the
/// target's parts had stopped, and Voice's onboarding commit, written as it
/// stopped, outlived the wipe, so the customer got no first five minutes.
/// The reset's stop names each part of the target, so each has stopped, and
/// written what it writes, before anything is wiped.
#[test]
fn a_reset_waits_for_every_part_of_the_target_before_wiping() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        parts: "lamp-voice.service lamp-memory.service\n".into(),
        writes_on_stop: Some(root.path("memory/progress.json")),
        ..Fake::default()
    };
    reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    let argvs = fake.argvs.borrow();
    assert_eq!(
        argvs[1],
        [
            "/bin/systemctl",
            "show",
            "-p",
            "ConsistsOf",
            "--value",
            "lampos.target"
        ]
        .map(String::from)
    );
    assert_eq!(
        argvs[3],
        [
            "/bin/systemctl",
            "stop",
            "lampos.target",
            "lamp-voice.service",
            "lamp-memory.service"
        ]
        .map(String::from)
    );
    assert_eq!(
        argvs[4],
        [
            "/bin/systemctl",
            "show",
            "-p",
            "ActiveState",
            "--value",
            "lamp-voice.service",
            "lamp-memory.service"
        ]
        .map(String::from),
        "each part is read stopped"
    );
    assert_eq!(argvs[5].last().map(String::as_str), Some("lampos.target"));
}

/// Reviewer-three's P2 on #3579 (the reviewer-lead's and Second's too):
/// setup serving in its 600 s after setup, an accepted helper's instance and
/// the skills' manager's write wiped paths, and none is part of
/// `lampos.target`. What starts them is stopped first, so none starts after
/// they are listed; then they are stopped with the target's parts and read
/// stopped. A helper that writes as it stops has written before the wipe.
#[test]
fn a_reset_stops_the_writers_outside_the_target_after_what_starts_them() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        parts: "lamp-voice.service".into(),
        starters: "lamp-skills-manage.socket loaded active listening Skills\nlamp-setup-change.socket loaded inactive dead Change\nlamp-update-back.timer loaded active waiting Back\nlamp-update-back.service loaded active running Back\n".into(),
        writers: "lamp-setup.service loaded active running Setup\nlamp-setup-change@3-1-0.service loaded active running Change\nlamp-setup-keys@9-9-9.service not-found inactive dead lamp-setup-keys@9-9-9.service\n\u{25cf} lamp-update.service loaded failed failed Update\n".into(),
        writer: Some("lamp-setup-change@3-1-0.service".into()),
        writes_on_stop: Some(root.path("memory/display-name")),
        ..Fake::default()
    };
    reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    let argvs = fake.argvs.borrow();
    let at = |verb: &str, word: &str| {
        argvs
            .iter()
            .position(|argv| argv[1] == verb && argv.iter().any(|w| w == word))
            .unwrap()
    };
    assert_eq!(argvs[0][6..], reset::STARTERS.map(String::from));
    assert_eq!(
        argvs[1],
        [
            "/bin/systemctl",
            "stop",
            "lamp-skills-manage.socket",
            "lamp-setup-change.socket",
            "lamp-update-back.timer",
            "lamp-update-back.service"
        ]
        .map(String::from),
        "what starts a writer, the updater's way back included, is stopped first"
    );
    assert!(at("list-units", "lamp-setup.service") > 1);
    assert_eq!(
        argvs[at("stop", "lampos.target")][2..],
        [
            "lampos.target",
            "lamp-voice.service",
            "lamp-setup.service",
            "lamp-setup-change@3-1-0.service",
            "lamp-update.service"
        ]
        .map(String::from),
        "a not-found stub is not named; a failed unit is"
    );
    assert_eq!(
        argvs[at("show", "ActiveState")][5..],
        [
            "lamp-skills-manage.socket",
            "lamp-setup-change.socket",
            "lamp-update-back.timer",
            "lamp-update-back.service",
            "lamp-voice.service",
            "lamp-setup.service",
            "lamp-setup-change@3-1-0.service",
            "lamp-update.service"
        ]
        .map(String::from),
        "the starters and the writers are read stopped"
    );
}

/// Reviewer-two's control on #3579: the updater's way back, a self-switch
/// pending, restarts `lamp-update.service` once it is stopped, past the final
/// check, and the updater writes the skills' state. The way back is stopped
/// first, before the barrier, so it restarts nothing and the wipe is the
/// last write.
#[test]
fn the_updaters_way_back_restarts_nothing_after_the_barrier() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        starters: "lamp-update-back.timer loaded active waiting Back\nlamp-update-back.service loaded active running Back\n".into(),
        writers: "lamp-update.service loaded active running Update\n".into(),
        back_worker: Some(root.path("memory/skills-state.json")),
        ..Fake::default()
    };
    reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    assert!(fake.back_stopped);
}

/// Third on #3579: Wi-Fi's rollback copy beside its file (`.lamp-previous`,
/// which the boot's restore puts back) and setup's staging copies go with
/// the file, so no later boot brings the factory's network back; and while a
/// reset is journalled, Wi-Fi puts no old network back.
#[test]
fn a_reset_wipes_the_wifi_copies_and_no_old_network_is_put_back_meanwhile() {
    let root = Root::new();
    let mut plan = lamp(&root);
    let conf = root.path("wpa_supplicant-wlan0.conf");
    plan.wipe.push(conf.clone());
    let copies = [
        "conf.lamp-previous",
        "conf.lamp-setup",
        "conf.lamp-setup-tmp",
    ]
    .map(|copy| conf.with_extension(copy));
    for file in copies.iter().chain([&conf]) {
        fs::write(file, b"network={ssid=\"factory\"}").unwrap();
    }
    // And setup's staging copy of a file it writes, cut mid-way (the
    // reviewer-lead's P3).
    let backend = root.path("backend.json");
    let staged = root.path("backend.lamp-setup-tmp");
    plan.wipe.push(backend.clone());
    fs::write(&backend, b"{}").unwrap();
    fs::write(&staged, b"{\"link\":\"the factory's\"}").unwrap();
    reset::reset(&plan, &mut Fake::default()).unwrap();
    wiped(&root);
    for file in copies.iter().chain([&conf, &backend, &staged]) {
        assert!(!file.exists(), "{} kept", file.display());
    }
    let journal = root.path("journal.json");
    assert!(reset::wifi_restore_allowed(&journal).unwrap());
    fs::write(&journal, b"{}").unwrap();
    assert!(!reset::wifi_restore_allowed(&journal).unwrap());
    fs::remove_file(&journal).unwrap();
    symlink(root.path("missing"), &journal).unwrap();
    assert!(!reset::wifi_restore_allowed(&journal).unwrap());
}

/// The parts cannot be read: as a stop that fails, nothing is wiped and the
/// lamp reboots into setup, which finishes the wipe with nothing running.
#[test]
fn parts_that_cannot_be_read_reboot_without_wiping() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        fail_read: true,
        ..Fake::default()
    };
    let steps = reset::reset(&plan, &mut fake).unwrap();
    assert_eq!(steps, [Step::Journal, Step::MarkerRemoved, Step::Rebooting]);
    assert_eq!(*fake.calls.borrow(), ["sync", "list-units", "reboot"]);
    assert!(root.path("setup/reset.json").exists());
    assert!(root.path("people.json").exists());
    reset::resume(&plan, &mut Fake::default()).unwrap();
    wiped(&root);
}

/// A part still running after the stop (Third's check on #1711), or one
/// that became part of the lamp during it: as a stop that fails, nothing is
/// wiped and the lamp reboots into setup, which finishes the wipe at its
/// start. So does a state missing from what systemd printed.
#[test]
fn a_part_not_stopped_or_one_that_joined_leaves_the_wipe_to_setup() {
    for fake in [
        Fake {
            parts: "lamp-voice.service lamp-memory.service".into(),
            states: Some("inactive\n\ndeactivating\n".into()),
            ..Fake::default()
        },
        Fake {
            parts: "lamp-voice.service lamp-memory.service".into(),
            states: Some("inactive\n".into()),
            ..Fake::default()
        },
        Fake {
            parts: "lamp-voice.service".into(),
            parts_after: Some("lamp-voice.service lamp-late.service".into()),
            ..Fake::default()
        },
        Fake {
            writers: "lamp-setup.service loaded active running Setup\n".into(),
            writers_after: Some(
                "lamp-setup.service loaded inactive dead Setup\nlamp-setup-keys@1-2-0.service loaded active running Keys\n"
                    .into(),
            ),
            ..Fake::default()
        },
    ] {
        let root = Root::new();
        let plan = lamp(&root);
        let mut fake = fake;
        let steps = reset::reset(&plan, &mut fake).unwrap();
        assert_eq!(steps, [Step::Journal, Step::MarkerRemoved, Step::Rebooting]);
        assert_eq!(
            fake.calls.borrow().last().map(String::as_str),
            Some("reboot")
        );
        assert!(root.path("setup/reset.json").exists());
        assert!(root.path("people.json").exists());
        reset::resume(&plan, &mut Fake::default()).unwrap();
        wiped(&root);
    }
}

/// The keeper's stop is not systemd's: it is run as the plan has it, and no
/// parts are asked for. A unit already named, or a printed word that reads
/// as an option, is not added; a part that is a target brings its own.
#[test]
fn only_a_systemctl_stop_of_a_target_names_its_parts() {
    let mut fake = Fake::default();
    let keeper: Vec<String> = ["/usr/bin/lamp-keeper", "stop", "lamp"]
        .map(String::from)
        .into();
    reset::stop_and_wait(&keeper, &mut fake).unwrap();
    assert_eq!(*fake.argvs.borrow(), [keeper]);
    let mut fake = Fake {
        parts: "lampos.target --now lamp-voice.service lamp-sub.target".into(),
        ..Fake::default()
    };
    let product: Vec<String> = ["/usr/bin/systemctl", "stop", "lampos.target"]
        .map(String::from)
        .into();
    reset::stop_and_wait(&product, &mut fake).unwrap();
    let argvs = fake.argvs.borrow();
    let stop = argvs.iter().find(|argv| argv[1] == "stop").unwrap();
    assert_eq!(
        *stop,
        [
            "/usr/bin/systemctl",
            "stop",
            "lampos.target",
            "lamp-voice.service",
            "lamp-sub.target",
            "lamp-sub-a.service"
        ]
        .map(String::from)
    );
}

#[test]
fn a_failed_stop_reboots_at_once_into_setup_which_finishes_the_wipe() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        fail_stop: true,
        ..Fake::default()
    };
    let steps = reset::reset(&plan, &mut fake).unwrap();
    assert_eq!(steps, [Step::Journal, Step::MarkerRemoved, Step::Rebooting]);
    assert_eq!(
        *fake.calls.borrow(),
        ["sync", "list-units", "show", "list-units", "stop", "reboot"]
    );
    // No marker, so the keeper starts only setup; the data is still there.
    assert!(!root.path("setup/done").exists());
    assert!(root.path("setup/reset.json").exists());
    assert!(root.path("people.json").exists());

    let mut start = Fake::default();
    let steps = reset::resume(&plan, &mut start).unwrap();
    wiped(&root);
    assert!(!steps.contains(&Step::Rebooting));
    assert_eq!(*start.calls.borrow(), ["sync", "sync"]);
    // Nothing left to finish.
    assert!(
        reset::resume(&plan, &mut Fake::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_wipe_that_fails_halfway_keeps_the_journal_until_setup_finishes_it() {
    let root = Root::new();
    let plan = lamp(&root);
    let locked = root.path("memory/locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("inside.json"), b"{}").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
    if root_user() {
        // Root ignores the mode this test takes away.
        return;
    }
    let mut fake = Fake::default();
    let error = reset::reset(&plan, &mut fake).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
    // It still reboots: a lamp left dark with no marker would wait for a hand.
    assert_eq!(
        fake.calls.borrow().last().map(String::as_str),
        Some("reboot")
    );
    // The marker went first, so a reboot now comes back into setup, and the
    // journal says the wipe is unfinished.
    assert!(root.path("setup/reset.json").exists());
    assert!(!root.path("setup/done").exists());

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    reset::resume(&plan, &mut Fake::default()).unwrap();
    wiped(&root);
}

#[test]
fn a_reboot_command_that_fails_falls_back_to_a_forced_reboot() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        fail_reboot: true,
        ..Fake::default()
    };
    reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    let calls = fake.calls.borrow();
    assert_eq!(calls[calls.len() - 2..], ["reboot", "force"]);
}

#[test]
fn a_link_on_the_way_to_a_wiped_path_is_never_followed() {
    let root = Root::new();
    lamp(&root);
    // `linked` points outside; a path through it must wipe nothing there.
    fs::create_dir_all(root.path("outside/store")).unwrap();
    fs::write(root.path("outside/store/keep.json"), b"{}").unwrap();
    symlink(root.path("outside"), root.path("linked")).unwrap();
    assert!(reset::wipe(&root.path("linked/store")).is_err());
    assert_eq!(
        fs::read(root.path("outside/store/keep.json")).unwrap(),
        b"{}"
    );
}

#[test]
fn without_a_journal_the_start_touches_nothing() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake::default();
    assert!(reset::resume(&plan, &mut fake).unwrap().is_empty());
    assert!(fake.calls.borrow().is_empty());
    assert!(root.path("people.json").exists());
    assert!(root.path("setup/done").exists());
}

#[test]
fn a_plan_never_wipes_software_system_or_shallow_paths() {
    let root = Root::new();
    let good = lamp(&root);
    good.check().unwrap();
    let with = |wipe: &str| Plan {
        wipe: vec![PathBuf::from(wipe)],
        ..good.clone()
    };
    for path in [
        "/opt/lampOS/voice/release",
        "/etc/lampOS/update/key.pub",
        "/etc/lampOS",
        "/usr/local/share/x",
        "/home/orangepi/data",
        "/var/lib",
        "/var",
        "var/lib/lampos-memory",
        "/var/lib/../lib/lampos-memory",
        "/var/lib/./lampos-memory",
        "/var/lib/lampos-memory/",
    ] {
        assert!(with(path).check().is_err(), "{path}");
    }
    assert!(with("/var/lib/lampos-memory").check().is_ok());
    let journal_inside = Plan {
        journal: good.wipe[0].join("reset.json"),
        ..good.clone()
    };
    assert!(journal_inside.check().is_err());
    let relative_command = Plan {
        stop: vec!["systemctl".into(), "stop".into()],
        ..good.clone()
    };
    assert!(relative_command.check().is_err());
    let nothing = Plan {
        wipe: vec![],
        ..good.clone()
    };
    assert!(nothing.check().is_err());
    // A refused plan touches nothing.
    assert!(reset::reset(&with("/var"), &mut Fake::default()).is_err());
    assert!(root.path("people.json").exists());
}

/// Every store of the person's that a part keeps, as lamp-4ace's units set
/// it or as the part's own default has it. A part that adds a store adds it
/// here and to the plan.
const PERSON_STORES: [&str; 39] = [
    // lamp-4ace's units.
    "/var/lib/lampos-memory",
    "/var/lib/lampos-eyes/people.json",
    // Interrupted ordinary saves and atomic repairs contain face prints too.
    "/var/lib/lampos-eyes/people.json.new",
    "/var/lib/lampos-eyes/people.json.repair.new",
    // Face prints the eyes could not read and set aside, never dropped
    // (#1505): a factory reset must not leave them behind.
    "/var/lib/lampos-eyes/set-aside",
    // Faces added from photos, not yet agreed to by their person, beside
    // the people's file (#1968): `photo::Unconfirmeds::beside`.
    "/var/lib/lampos-eyes/people.json.from-photos",
    // The face reads of a labelled session (`lamp-eyes --seems-log`, #1671),
    // the one path the eyes take for it (`seems_log::PATH`).
    "/var/lib/lampos-eyes/seems.log",
    // When a face was at the lamp, glance by glance, for the change bar's
    // replay (`lamp-eyes --glance-log`), the one path the eyes take for it
    // (`glance_log::PATH`).
    "/var/lib/lampos-eyes/glance.log",
    "/var/lib/lampos-tasks/store",
    "/var/lib/lampos-tasks/targets.json",
    // The person's parts of Harness under the tasks part's HOME (the fifth
    // on #1016): the sign-in, the pairings and what they left. Not
    // `.harness` whole, which is Harness itself, the CLI and its own Node
    // (Nine on #1016, 16:0x, from a real install on lamp-75c9).
    "/var/lib/lampos-tasks/.harness/auth",
    "/var/lib/lampos-tasks/.harness/connections",
    "/var/lib/lampos-tasks/.harness/computer-id",
    "/var/lib/lampos-tasks/.harness/cli/data",
    "/var/lib/lampos-tasks/.harness/agent-messages",
    "/var/lib/lampos-tasks/.harness/hook-routes",
    "/var/lib/lampos-tasks/.harness/logs",
    "/var/lib/lampos-schedule/store",
    // Daily routine claims and durable output restrictions belong to this
    // owner even when their switches are currently off.
    "/var/lib/lampos-skills-state",
    // The skills a paired computer pushed, and the owner's yes to any that
    // sees the picture, kept beside them (piece B, Reviewer-three's P2 on
    // #4514): the next owner's lamp has neither.
    "/var/lib/lampOS/skills-dev",
    "/var/lib/lampos-output-caps",
    "/var/lib/lampos-policy",
    "/var/lib/lampOS/voice/sessions",
    "/var/lib/lampOS/voice/onboarding",
    // The person's yes to music and laughter on a lamp with no privacy
    // slide, read by lamp-ears (#1722 item 1, docs/onboarding.md).
    "/var/lib/lampOS/consent/room-sounds.json",
    "/var/lib/lampOS/consent/face-expressions.json",
    "/var/lib/lampOS/voice/schedule-delivery",
    "/var/lib/lampOS/voice/first-output",
    "/run/lampos-voice/thread.json",
    // The voice's default for the task news it has told (reviewer, #817):
    // `DEFAULT_TOLD` in crates/voice/src/task_news.rs.
    "/var/lib/lampOS/voice/tasks-told",
    // Skills made by voice, kept unconfirmed (`LAMP_MAKE_SKILL`, #2011): their
    // names and words come from the person's requests (the reviewer's P2-2
    // on #2011), `made_skill::DIRECTORY` in crates/voice/src/made_skill.rs.
    "/var/lib/lampOS/voice/made-skills",
    // The person's light the ring keeps with `LAMP_LIGHT_KEEP=on` (#1468):
    // the folder, so a `person.json.new` a power cut left goes too.
    "/var/lib/lampOS/light",
    // What setup saves, and its marker: the user's key, to the voice and to
    // the Policy (the owner, #806, 16:13), and the name.
    "/etc/lampOS/voice/credential",
    // The person's own DeepSeek and Deepgram keys (#2094), the folder the
    // voice loads whole: `keys::FOLDER` in crates/setup/src/keys.rs.
    "/etc/lampOS/voice/own",
    "/etc/lampOS/policy/jev.key",
    "/etc/lampOS/person/name",
    // The owner's place from the app, beside the name (#4333).
    "/etc/lampOS/person/place",
    "/etc/wpa_supplicant/wpa_supplicant-wlan0.conf",
    "/var/lib/lampos-setup/done",
];

#[test]
fn the_example_plan_wipes_every_store_of_the_person() {
    let plan: Plan =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-example.json")).unwrap();
    plan.check().unwrap();
    for store in PERSON_STORES.map(Path::new) {
        assert!(
            plan.marker == store || plan.wipe.iter().any(|wiped| store.starts_with(wiped)),
            "{} is not in the reset plan",
            store.display()
        );
    }
    // What makes the lamp itself is kept: its update key.
    let kept = Path::new("/etc/lampOS/update/key.pub");
    assert!(
        !plan.wipe.iter().any(|wiped| kept.starts_with(wiped)),
        "{} is wiped",
        kept.display()
    );
}

#[test]
fn current_reset_plans_wipe_people_and_both_temporaries_without_widening_the_directory() {
    let home = Path::new("/var/lib/lampos-eyes");
    let files = ["people.json", "people.json.new", "people.json.repair.new"];
    for source in [
        include_str!("../../fixtures/reset-plan-product.json"),
        include_str!("../../fixtures/reset-plan-example.json"),
        include_str!("../../../../deploy/lamp-06c1/reset-plan.json"),
    ] {
        let plan: Plan = serde_json::from_str(source).unwrap();
        plan.check().unwrap();
        for file in files {
            assert!(plan.wipe.contains(&home.join(file)), "missing {file}");
        }
        assert!(!plan.wipe.iter().any(|path| home.starts_with(path)));
        let root = Root::new();
        let directory = root.0.join(home.strip_prefix("/").unwrap());
        fs::create_dir_all(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        for file in files {
            fs::write(directory.join(file), b"synthetic private face state").unwrap();
        }
        let kept = directory.join("people.json.repair.new.keep");
        fs::write(&kept, b"unrelated synthetic setting").unwrap();
        for path in &plan.wipe {
            reset::wipe(&root.0.join(path.strip_prefix("/").unwrap())).unwrap();
        }
        for file in files {
            assert!(
                fs::symlink_metadata(directory.join(file)).is_err(),
                "{file}"
            );
        }
        assert_eq!(fs::read(kept).unwrap(), b"unrelated synthetic setting");
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

/// Harness's real layout under the tasks part's home (Nine on #1016, from
/// installing it on lamp-75c9): the product plan's Harness paths, run by
/// the real wipe, take the previous owner's sign-in, pairings and logs and
/// leave Harness itself (its CLI, its own Node, its wrapper) able to run.
#[test]
fn a_reset_takes_harness_sign_in_and_pairings_and_keeps_harness_itself() {
    let plan: Plan =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    let root = Root::new();
    let harness = root.path(".harness");
    let files = [
        // Harness itself.
        ("cli/cli.js", true),
        ("runtime/node-v22/bin/node", true),
        ("bin/harness", true),
        // The person's.
        ("auth/session.json", false),
        ("connections/office-mac.json", false),
        ("computer-id", false),
        ("cli/data/autonomous-device-connections.json", false),
        ("agent-messages/1.json", false),
        ("hook-routes/claude.json", false),
        ("logs/harnessd.log", false),
    ];
    for (file, _) in files {
        let path = harness.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
    }
    let home = Path::new("/var/lib/lampos-tasks/.harness");
    let mut wiped_here = 0;
    for path in &plan.wipe {
        if let Ok(part) = path.strip_prefix(home) {
            reset::wipe(&harness.join(part)).unwrap();
            wiped_here += 1;
        }
    }
    assert_eq!(wiped_here, 7);
    assert!(
        !plan.wipe.iter().any(|path| path == home),
        "not .harness whole"
    );
    for (file, kept) in files {
        assert_eq!(harness.join(file).exists(), kept, "{file}");
    }
}

/// Every folder the product keeps across a boot, the tmpfiles' `d` and `D`
/// lines and each product unit's `StateDirectory=`, is the reset's: the
/// shipped plan wipes it, a folder above it, or what in it is the person's
/// (its marker and journal count). The rest is on [`KEPT`], each with why. A
/// new data folder fails this until the plan or the list names it, so it
/// fails on a Mac, not after a reset on a lamp (Reviewer-three's P3 beside
/// #4542: `/var/lib/lampOS/skills-dev` on #4514).
#[test]
fn every_product_data_folder_is_wiped_by_the_reset_or_kept_for_a_reason() {
    const KEPT: [(&str, &str); 4] = [
        // The factory's self-test record, kept on purpose (install.md).
        ("/var/lib/lamp-selftest", "the self-test's runs"),
        // The updater's records: the builds, its state, its lock.
        ("/var/lib/lampos-update", "the update's records"),
        // PulseAudio's client setting (#4487), no one's data.
        ("/etc/pulse", "the image's sound setting"),
        ("/etc/pulse/client.conf.d", "the image's sound setting"),
    ];
    let plan: Plan =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let mut folders: Vec<String> = fs::read_to_string(product.join("lampos.conf"))
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("d ").or_else(|| line.strip_prefix("D ")))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter(|folder| !folder.starts_with("/run/"))
        .map(String::from)
        .collect();
    for entry in fs::read_dir(&product).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|kind| kind != "service") {
            continue;
        }
        for line in fs::read_to_string(&path).unwrap().lines() {
            if let Some(names) = line.trim().strip_prefix("StateDirectory=") {
                folders.extend(
                    names
                        .split_whitespace()
                        .map(|name| format!("/var/lib/{name}")),
                );
            }
        }
    }
    folders.sort();
    folders.dedup();
    assert!(
        folders
            .iter()
            .any(|folder| folder == "/var/lib/lampos-memory"),
        "the product's folders were read: {folders:?}"
    );
    for (kept, why) in KEPT {
        assert!(
            folders.iter().any(|folder| folder == kept),
            "{kept} ({why}) is no longer a product folder: take it off the list"
        );
    }
    let wiped: Vec<&Path> = plan
        .wipe
        .iter()
        .chain([&plan.marker, &plan.journal])
        .map(PathBuf::as_path)
        .collect();
    let unnamed: Vec<&String> = folders
        .iter()
        .filter(|folder| {
            let folder = Path::new(folder.as_str());
            !wiped
                .iter()
                .any(|path| folder.starts_with(path) || path.starts_with(folder))
        })
        .filter(|folder| !KEPT.iter().any(|(kept, _)| kept == folder))
        .collect();
    assert!(
        unnamed.is_empty(),
        "kept across a boot, neither wiped by the product's reset plan nor on its keep list: {unnamed:?}"
    );
}

/// Reset covers the whole private onboarding directory, including unfinished
/// writes and handoffs, while the installed core skill is software to keep.
#[test]
fn reset_wipes_all_onboarding_state_and_keeps_the_core_skill() {
    let home = Path::new("/var/lib/lampOS/voice/onboarding");
    let skill = Path::new("/var/lib/lampOS/skills/onboarding/skill.json");
    for source in [
        include_str!("../../fixtures/reset-plan-product.json"),
        include_str!("../../fixtures/reset-plan-example.json"),
    ] {
        let plan: Plan = serde_json::from_str(source).unwrap();
        plan.check().unwrap();
        assert!(plan.wipe.iter().any(|path| path == home));
        assert!(!plan.wipe.iter().any(|path| skill.starts_with(path)));
        let root = Root::new();
        let private = root.path(home.strip_prefix("/").unwrap().to_str().unwrap());
        let package = root.path(skill.strip_prefix("/").unwrap().to_str().unwrap());
        for file in [
            "progress.json",
            "privacy.json",
            "progress.json.pending",
            ".checkpoint.tmp",
            "mailbox/receipt.json",
        ] {
            let path = private.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"private state").unwrap();
        }
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir_all(package.parent().unwrap()).unwrap();
        fs::write(&package, b"core skill").unwrap();
        // An interrupted writer's link cannot make reset erase the package.
        symlink(&package, private.join("package-link")).unwrap();
        for path in &plan.wipe {
            let mapped = root.0.join(path.strip_prefix("/").unwrap());
            reset::wipe(&mapped).unwrap();
        }
        assert!(private.is_dir(), "reset retains the private container");
        assert_eq!(fs::read_dir(&private).unwrap().count(), 0);
        assert_eq!(
            fs::metadata(&private).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(fs::read(&package).unwrap(), b"core skill");
    }
}

/// A lamp put on by `lamp-update install` has no keeper (install.md, "Setup
/// at first boot, without the keeper"): its plan stops the lamp with
/// systemd, and is the example's in everything else, so the two never drift.
#[test]
fn the_product_plan_is_the_example_stopped_by_systemd() {
    let example: Plan =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-example.json")).unwrap();
    let product: Plan =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    product.check().unwrap();
    assert_eq!(
        product.stop,
        ["/usr/bin/systemctl", "stop", "lampos.target"]
    );
    assert_eq!(
        Plan {
            stop: example.stop.clone(),
            ..product
        },
        example
    );
}

#[test]
fn the_product_reset_wipes_daily_claims_and_caps_but_keeps_service_directories() {
    let plan: Plan =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    plan.check().unwrap();
    let root = Root::new();
    let stores = ["lampos-skills-state", "lampos-output-caps"];
    for store in stores {
        let path = root.path(store);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        // Wipe the whole store, including a write interrupted by power loss
        // and any state kept aside, rather than one currently known filename.
        for file in ["state", "state.new", "set-aside/previous"] {
            let file = path.join(file);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"previous owner's state").unwrap();
        }
    }
    fs::write(root.path("lampos-skills-state/daily.json"), b"{}").unwrap();
    fs::write(
        root.path("lampos-output-caps/schedule.sqlite3"),
        b"snapshot",
    )
    .unwrap();
    fs::write(root.path("kept-setting"), b"kept").unwrap();
    for store in stores {
        let installed = Path::new("/var/lib").join(store);
        assert!(
            plan.wipe.contains(&installed),
            "{store} is absent from the product plan"
        );
        let local = root.path(store);
        reset::wipe(&local).unwrap();
        assert!(local.is_dir());
        assert_eq!(fs::read_dir(&local).unwrap().count(), 0);
        assert_eq!(
            fs::metadata(&local).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    assert_eq!(fs::read(root.path("kept-setting")).unwrap(), b"kept");
}

/// A pushed skill and the owner's yes to its picture are the owner's: the
/// product reset wipes the developer folder they're kept in, and keeps the
/// folder itself (Reviewer-three's P2 on #4514).
#[test]
fn the_product_reset_wipes_pushed_skills_and_their_picture_yes() {
    let plan: Plan =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    plan.check().unwrap();
    assert!(
        plan.wipe
            .contains(&Path::new("/var/lib/lampOS/skills-dev").to_path_buf()),
        "the developer folder is absent from the product plan"
    );
    let root = Root::new();
    let dev = root.path("skills-dev");
    let skill = dev.join("sees-desk");
    fs::create_dir_all(&skill).unwrap();
    for (file, text) in [
        ("SKILL.md", "---\nname: sees-desk\n---\n"),
        ("skill.toml", "format = 2\n"),
        (".pushed.json", "{\"client\": \"c_1\"}"),
    ] {
        fs::write(skill.join(file), text).unwrap();
    }
    fs::write(
        dev.join(".yes.json"),
        b"{\"c_1\": {\"sees-desk\": \"sha\"}}",
    )
    .unwrap();
    reset::wipe(&dev).unwrap();
    assert!(dev.is_dir());
    assert_eq!(fs::read_dir(&dev).unwrap().count(), 0);
}

fn event(at_us: u64, held_us: u64) -> TouchEvent {
    TouchEvent {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us,
            sequence: 1,
        },
        session_id: "controls-session".into(),
        reader_session_id: "gpio-reader".into(),
        enabled: true,
        source: TouchSource::Reset,
        name: "reset".into(),
        contact_id: 1,
        gesture: TouchGesture::Hold,
        when_speaking: TouchIntent::FactoryReset,
        when_quiet: TouchIntent::FactoryReset,
        when_asleep: Some(TouchIntent::FactoryReset),
        began_us: at_us - held_us,
        observed_us: at_us,
        released_us: Some(at_us),
        active_mask: 0,
        active_names: vec![],
        position: None,
        toward_pad: None,
        distance: 0.0,
        speed: 0.0,
    }
}

#[test]
fn only_an_enabled_fresh_release_after_five_seconds_on_this_boot_is_a_reset() {
    let at = 20_000_000;
    let good = event(at, 5_000_000);
    assert_eq!(accept(&good, CLOCK, at + 1_000), Ok(()));
    assert!(accept(&event(at, 4_999_999), CLOCK, at).is_err());
    let mut shadow = good.clone();
    shadow.enabled = false;
    assert_eq!(accept(&shadow, CLOCK, at), Err("controls is in shadow"));
    assert_eq!(
        accept(&good, "another-boot", at),
        Err("another boot's clock")
    );
    assert_eq!(accept(&good, CLOCK, at + 500_000), Err("stale"));
    assert_eq!(accept(&good, CLOCK, at - 1), Err("from the future"));
    let mut primary = good.clone();
    primary.source = TouchSource::Primary;
    primary.when_speaking = TouchIntent::SoftwareMute;
    primary.when_quiet = TouchIntent::SoftwareMute;
    primary.when_asleep = Some(TouchIntent::SoftwareMute);
    assert!(accept(&primary, CLOCK, at).is_err());
}

fn publish_and_watch(enabled: bool, events: Vec<TouchEvent>) -> (Watched, usize) {
    publish_and_watch_as(false, enabled, events)
}

/// With `refused`, the watcher of a plan this build refuses: what it acts
/// on is the telling, counted the same.
fn publish_and_watch_as(refused: bool, enabled: bool, events: Vec<TouchEvent>) -> (Watched, usize) {
    // A short root: a Unix socket's address holds 103 bytes.
    let root = PathBuf::from(format!(
        "/tmp/ls-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _clean = Root(root.clone());
    let bus = Bus::new(&root).unwrap();
    let mut touch = bus
        .subscribe_typed(TouchEvent::TOPIC, "setup", TouchEvent::TYPE)
        .unwrap();
    let mut publisher = bus
        .publisher_declared(TouchEvent::TOPIC, &TouchEvent::declaration(), "controls")
        .unwrap();
    // The subscriber's socket is in `subs/` before the publisher is made,
    // and a new publisher reads `subs/` at its first publish: no wait.
    for event in &events {
        publisher.publish_json(event.stamp.at_us, event).unwrap();
    }
    let acted = RefCell::new(0);
    let logged = RefCell::new(Vec::new());
    let now = events.last().map_or(0, |event| event.stamp.at_us + 1_000);
    let until = Some(Instant::now() + Duration::from_millis(600));
    let log = |line: &str| logged.borrow_mut().push(line.to_owned());
    let watched = if refused {
        lamp_setup::watch_refused(
            &mut touch,
            CLOCK,
            enabled,
            until,
            || now,
            log,
            None,
            || {
                *acted.borrow_mut() += 1;
            },
        )
    } else {
        lamp_setup::watch(
            &mut touch,
            CLOCK,
            enabled,
            until,
            || now,
            log,
            None,
            || {
                *acted.borrow_mut() += 1;
                Ok(())
            },
        )
    }
    .unwrap();
    LOGGED.with(|log| *log.borrow_mut() = logged.into_inner());
    (watched, acted.into_inner())
}

thread_local! {
    static LOGGED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[test]
fn the_watcher_acts_once_on_a_real_bus_and_only_logs_with_the_switch_off() {
    let at = 30_000_000;
    // Passed over: one too old, one held under 5 s. Then a good one.
    let stale = event(at - 600_000, 5_000_000);
    let mut short = event(at - 2_000, 4_000_000);
    short.contact_id = 2;
    let mut good = event(at, 6_000_000);
    good.contact_id = 3;
    assert_eq!(
        publish_and_watch(true, vec![stale, short, good.clone()]),
        (Watched::Reset, 1)
    );
    assert_eq!(
        publish_and_watch(false, vec![good.clone()]),
        (Watched::Shadow, 0)
    );
    let mut shadow_controls = good.clone();
    shadow_controls.enabled = false;
    assert_eq!(
        publish_and_watch(true, vec![shadow_controls]),
        (Watched::Ended, 0)
    );
    // The event did arrive, and was passed over for its reason.
    LOGGED.with(|log| {
        assert!(
            log.borrow()
                .iter()
                .any(|line| line.contains("controls is in shadow")),
            "{:?}",
            log.borrow()
        );
    });
}

#[test]
fn the_watcher_of_a_refused_plan_tells_each_reset_and_keeps_watching() {
    let at = 40_000_000;
    let first = event(at - 1_000, 6_000_000);
    let mut second = event(at, 6_000_000);
    second.contact_id = 2;
    assert_eq!(
        publish_and_watch_as(true, true, vec![first, second.clone()]),
        (Watched::Refused, 2)
    );
    LOGGED.with(|log| {
        let log = log.borrow();
        assert_eq!(
            log.iter()
                .filter(|line| line.contains("nothing was wiped"))
                .count(),
            2,
            "{log:?}"
        );
        assert!(!log.iter().any(|line| line.contains("wiping")), "{log:?}");
    });
    assert_eq!(
        publish_and_watch_as(true, false, vec![second]),
        (Watched::Shadow, 0)
    );
}

#[test]
fn the_command_checks_plans_and_reports_an_unfinished_reset_without_acting() {
    let root = Root::new();
    let plan = lamp(&root);
    let file = root.path("plan.json");
    fs::write(&file, serde_json::to_vec(&plan).unwrap()).unwrap();
    let run = |args: &[&str]| Command::new(PROGRAM).args(args).output().unwrap();
    let checked = run(&["check", "--plan", file.to_str().unwrap()]);
    assert!(checked.status.success(), "{checked:?}");
    let off = run(&["reset-now", "--plan", file.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&off.stdout).contains("\"enabled\":false"));
    assert!(
        root.path("people.json").exists(),
        "reset-now does nothing without --enable"
    );
    fs::write(root.path("setup/reset.json"), b"{}").unwrap();
    let shadow = run(&["resume-reset", "--plan", file.to_str().unwrap()]);
    assert!(shadow.status.success());
    assert!(String::from_utf8_lossy(&shadow.stdout).contains("\"unfinished\":true"));
    assert!(
        root.path("people.json").exists(),
        "nothing wiped without --enable"
    );
    let mut bad = plan.clone();
    bad.wipe = vec![Path::new("/opt/lampOS/voice").into()];
    fs::write(&file, serde_json::to_vec(&bad).unwrap()).unwrap();
    assert_eq!(
        run(&["check", "--plan", file.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
}

/// A plan a build one version ahead writes (#1680, the rule agreed on
/// #2060), from this build's plan with `more` merged over it, its stop and
/// reboot harmless so a reset this build wrongly runs only wipes.
fn ahead(plan: &Plan, more: serde_json::Value) -> serde_json::Value {
    let mut value = serde_json::to_value(plan).unwrap();
    value["stop"] = serde_json::json!(["/usr/bin/true"]);
    value["reboot"] = serde_json::json!(["/usr/bin/true"]);
    for (field, more) in more.as_object().unwrap() {
        value[field] = more.clone();
    }
    value
}

/// Run `command --plan … --enable` on `plan`, as the lamp does.
fn act_on(root: &Root, command: &str, plan: &serde_json::Value) -> std::process::Output {
    let file = root.path("plan.json");
    fs::write(&file, serde_json::to_vec(plan).unwrap()).unwrap();
    Command::new(PROGRAM)
        .args([command, "--plan", file.to_str().unwrap(), "--enable"])
        .output()
        .unwrap()
}

/// Nothing of the reset happened: no journal, the marker kept, nothing wiped.
fn untouched(root: &Root) {
    assert!(!root.path("setup/reset.json").exists(), "no journal");
    assert!(root.path("setup/done").exists(), "the marker kept");
    assert!(root.path("memory/index.json").exists(), "nothing wiped");
    assert!(root.path("tasks/1.json").exists(), "nothing wiped");
    assert!(root.path("people.json").exists(), "nothing wiped");
    assert!(root.path("people.json.new").exists(), "nothing wiped");
    assert!(
        root.path("people.json.repair.new").exists(),
        "nothing wiped"
    );
}

#[test]
fn an_unknown_kind_in_wipe_refuses_the_reset_with_nothing_wiped() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut value = ahead(&plan, serde_json::json!({}));
    value["wipe"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"kind": "tree", "path": root.path("voice")}));
    assert!(matches!(
        reset::read(&serde_json::to_vec(&value).unwrap()),
        Err(reset::Refused::Ahead(why)) if why.contains("wipe entry 8")
    ));
    let out = act_on(&root, "reset-now", &value);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("wipe entry 8"), "{said}");
    assert!(said.contains("nothing is wiped"), "{said}");
    assert!(said.contains("update the lamp, then reset again"), "{said}");
    untouched(&root);
}

#[test]
fn a_newer_plans_extra_plain_path_is_wiped() {
    let root = Root::new();
    let plan = lamp(&root);
    // A newer build's new store, named as a plain path, with an empty
    // keep-back field beside it: read with nothing passed over.
    fs::create_dir_all(root.path("voice")).unwrap();
    fs::write(root.path("voice/key"), b"k").unwrap();
    let mut value = ahead(&plan, serde_json::json!({"keep_back": []}));
    value["wipe"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(root.path("voice")));
    let (read, notes) = reset::read(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(read.wipe.contains(&root.path("voice")));
    assert!(notes.is_empty(), "{notes:?}");
    fs::write(root.path("setup/reset.json"), b"{}").unwrap();
    let out = act_on(&root, "resume-reset", &value);
    assert!(out.status.success(), "{out:?}");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("dropped"),
        "{out:?}"
    );
    wiped(&root);
    assert!(!root.path("voice/key").exists(), "the newer path is wiped");
    // A newer plan's extra path is still checked against what is never wiped.
    value["wipe"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("/opt/lampOS/voice"));
    assert!(matches!(
        reset::read(&serde_json::to_vec(&value).unwrap()),
        Err(reset::Refused::Bad(_))
    ));
}

#[test]
fn an_unknown_keep_back_entry_is_dropped_and_its_path_wiped() {
    let root = Root::new();
    let plan = lamp(&root);
    let value = ahead(
        &plan,
        serde_json::json!({"keep_back": [
            {"kind": "all_but", "path": root.path("memory"), "keep": ["index.json"]},
            root.path("memory/index.json"),
        ]}),
    );
    let (read, notes) = reset::read(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(read.wipe, plan.wipe);
    assert_eq!(notes.len(), 2, "{notes:?}");
    fs::write(root.path("setup/reset.json"), b"{}").unwrap();
    let out = act_on(&root, "resume-reset", &value);
    assert!(out.status.success(), "{out:?}");
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("keep_back entry 1 is not one"), "{said}");
    assert!(said.contains("keep_back entry 2 is not one"), "{said}");
    wiped(&root);
    assert!(
        !root.path("memory/index.json").exists(),
        "what it would keep is wiped"
    );
}

#[test]
fn an_unknown_field_refuses_the_reset_with_nothing_wiped() {
    let root = Root::new();
    let plan = lamp(&root);
    let value = ahead(
        &plan,
        serde_json::json!({"stop_first": ["/usr/bin/true", "voice"]}),
    );
    assert!(matches!(
        reset::read(&serde_json::to_vec(&value).unwrap()),
        Err(reset::Refused::Ahead(why)) if why.contains("`stop_first`")
    ));
    let out = act_on(&root, "reset-now", &value);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("`stop_first`"),
        "{out:?}"
    );
    untouched(&root);
}

#[test]
fn a_plan_ahead_still_refuses_a_bad_known_value() {
    let root = Root::new();
    let plan = lamp(&root);
    let bad = |more: serde_json::Value| {
        let value = ahead(&plan, more);
        matches!(
            reset::read(&serde_json::to_vec(&value).unwrap()),
            Err(reset::Refused::Bad(_))
        )
    };
    assert!(bad(serde_json::json!({"wipe": ["/opt/lampOS/voice"]})));
    assert!(bad(serde_json::json!({"marker": 7})));
    assert!(bad(
        serde_json::json!({"keep_back": {"path": "/var/lib/x"}})
    ));
    assert!(bad(serde_json::json!({"wipe": []})));
    assert!(matches!(reset::read(b"{"), Err(reset::Refused::Bad(_))));
}

#[test]
fn the_plan_reader_knows_every_field_of_the_plan() {
    let root = Root::new();
    let value = serde_json::to_value(lamp(&root)).unwrap();
    let mut fields: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    let mut known = reset::FIELDS.to_vec();
    known.sort_unstable();
    assert_eq!(fields, known);
}

/// #2094: the reset wipes the person's own keys, the folder as one plain
/// path, and the voice's folder stays for the next setup.
#[test]
fn a_reset_wipes_the_persons_own_keys() {
    let product: Plan =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    let folder = Path::new(lamp_setup::keys::FOLDER);
    assert!(product.wipe.iter().any(|path| path == folder));
    let root = Root::new();
    let own = root.path("own");
    fs::create_dir_all(&own).unwrap();
    for file in ["deepseek.key", "deepseek.url", "deepgram.key"] {
        fs::write(own.join(file), b"sk-planted-2094").unwrap();
    }
    reset::wipe(&own).unwrap();
    assert!(own.is_dir());
    assert_eq!(fs::read_dir(&own).unwrap().count(), 0);
}

/// #806: `lamp-setup check` reads the plan install and a round place
/// (`lamp-update`'s `install::RESET_PLAN` is this file), with the stores the
/// 19 Oct plan lacked.
#[test]
fn check_reads_the_product_plan_install_places() {
    let checked = Command::new(PROGRAM)
        .args([
            "check",
            "--plan",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/reset-plan-product.json"
            ),
        ])
        .output()
        .unwrap();
    assert!(checked.status.success(), "{checked:?}");
    let plan: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    let wipe = plan["wipe"].as_array().unwrap();
    for store in ["/etc/lampOS/voice/own", "/etc/lampOS/setup/display-name"] {
        assert!(wipe.iter().any(|path| path == store), "{store}: {wipe:?}");
    }
}

/// #3710: lamp-setup-reset.service is read-only but the bus, so an enabled
/// watcher whose plan has no `detach` would run the reset in there and wipe
/// nothing after the button. It is refused at start instead, before the bus
/// is touched (Fourteen's P3 on #4048).
#[test]
fn an_enabled_watcher_refuses_a_plan_without_detach_at_its_start() {
    let root = Root::new();
    let mut plan: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/reset-plan-product.json")).unwrap();
    plan.as_object_mut().unwrap().remove("detach");
    let file = root.path("reset-plan.json");
    fs::write(&file, serde_json::to_vec(&plan).unwrap()).unwrap();
    let bus = root.path("bus");
    let out = Command::new(PROGRAM)
        .args(["watch-reset", "--plan"])
        .arg(&file)
        .arg("--bus-root")
        .arg(&bus)
        .arg("--enable")
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("needs the plan's detach"), "{said}");
    assert!(!bus.exists(), "the bus was touched before the refusal");
}

/// A request to turn the lamp off, from `source`, stamped `at_us`.
fn request(at_us: u64, source: lamp_msgs::ShutdownSource) -> lamp_msgs::ShutdownRequest {
    lamp_msgs::ShutdownRequest {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us,
            sequence: 1,
        },
        source,
        reason: "primary button held 6000 ms".into(),
    }
}

/// Publish `requests` (each under its bus publisher name) on the shutdown's
/// topic, and watch with the switch `on` or off and `--enable`: how many
/// shutdowns were started, and the log. The runner takes each start and is
/// still at work after it.
fn watch_requests(
    on: bool,
    enabled: bool,
    requests: Vec<(&str, lamp_msgs::ShutdownRequest)>,
) -> (usize, Vec<String>) {
    watch_requests_taken(on, enabled, true, requests)
}

/// As [`watch_requests`], the runner `taken`: still at work after a start,
/// or (false) refused at once, as while an update installs, and gone.
fn watch_requests_taken(
    on: bool,
    enabled: bool,
    taken: bool,
    requests: Vec<(&str, lamp_msgs::ShutdownRequest)>,
) -> (usize, Vec<String>) {
    use lamp_msgs::ShutdownRequest;
    let root = PathBuf::from(format!(
        "/tmp/ls-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _clean = Root(root.clone());
    let bus = Bus::new(&root).unwrap();
    let mut touch = bus
        .subscribe_typed(TouchEvent::TOPIC, "setup", TouchEvent::TYPE)
        .unwrap();
    let mut asked = bus
        .subscribe_typed(ShutdownRequest::TOPIC, "setup", ShutdownRequest::TYPE)
        .unwrap();
    let now = requests
        .iter()
        .map(|(_, request)| request.stamp.at_us)
        .max()
        .unwrap_or(0)
        + 1_000;
    for (publisher, request) in &requests {
        bus.publisher_declared(
            ShutdownRequest::TOPIC,
            &ShutdownRequest::declaration(),
            publisher,
        )
        .unwrap()
        .publish_json(request.stamp.at_us, request)
        .unwrap();
    }
    let started = std::cell::Cell::new(0);
    let mut start = || {
        started.set(started.get() + 1);
        Ok(())
    };
    let mut running = || taken && started.get() > 0;
    let logged = RefCell::new(Vec::new());
    lamp_setup::watch(
        &mut touch,
        CLOCK,
        enabled,
        Some(Instant::now() + Duration::from_millis(600)),
        || now,
        |line: &str| logged.borrow_mut().push(line.to_owned()),
        Some(lamp_setup::ShutdownWatch {
            requests: &mut asked,
            start: on.then_some(&mut start as &mut dyn FnMut() -> io::Result<()>),
            running: &mut running,
        }),
        || panic!("no reset was asked"),
    )
    .unwrap();
    (started.get(), logged.into_inner())
}

#[test]
fn the_watcher_starts_the_shutdown_once_only_with_the_switch_on_and_fresh() {
    use lamp_msgs::ShutdownSource::Button;
    let at = 50_000_000;
    // Two valid requests: the shutdown is started once.
    let (started, log) = watch_requests(
        true,
        true,
        vec![
            ("controls", request(at - 1_000, Button)),
            ("controls", request(at, Button)),
        ],
    );
    assert_eq!(started, 1, "{log:?}");
    assert!(
        log.iter().any(|line| line.contains("already turning off")),
        "{log:?}"
    );
    // Off, or without --enable: logged, never started.
    let (started, log) = watch_requests(false, true, vec![("controls", request(at, Button))]);
    assert_eq!(started, 0);
    assert!(
        log.iter()
            .any(|line| line.contains("LAMP_SHUTDOWN is not on")),
        "{log:?}"
    );
    assert_eq!(
        watch_requests(true, false, vec![("controls", request(at, Button))]).0,
        0
    );
    // Stale, from another boot, or not from its source's publisher.
    let (started, log) = watch_requests(
        true,
        true,
        // The second, never taken, sets the watcher's clock to `at`.
        vec![
            ("controls", request(at - 3_000_000, Button)),
            ("voice", request(at, Button)),
        ],
    );
    assert_eq!(started, 0);
    assert!(log.iter().any(|line| line.contains("stale")), "{log:?}");
    let mut foreign = request(at, Button);
    foreign.stamp.clock_id = "another-boot".into();
    assert_eq!(watch_requests(true, true, vec![("controls", foreign)]).0, 0);
    let (started, log) = watch_requests(true, true, vec![("cloud", request(at, Button))]);
    assert_eq!(started, 0);
    assert!(
        log.iter()
            .any(|line| line.contains("not from its source's publisher")),
        "{log:?}"
    );
    assert_eq!(
        watch_requests(true, true, vec![("voice", request(at, Button))]).0,
        0
    );
}

/// The Reviewer-lead's P2 on #3729: `systemd-run` returning only hands the
/// shutdown to the runner, which refuses while an update installs and is
/// gone. A flag set on that return passed over every later hold this boot;
/// the watcher now asks whether the runner is still at work, so the second
/// hold, once the update is done, turns the lamp off.
#[test]
fn a_refused_shutdown_leaves_the_next_hold_acted_on() {
    use lamp_msgs::ShutdownSource::Button;
    let at = 50_000_000;
    let (started, log) = watch_requests_taken(
        true,
        true,
        false,
        vec![
            ("controls", request(at - 1_000, Button)),
            ("controls", request(at, Button)),
        ],
    );
    assert_eq!(started, 2, "{log:?}");
    assert!(
        !log.iter().any(|line| line.contains("already turning off")),
        "{log:?}"
    );
}

/// #4500 (lamp-cb94, 30 Oct): the A733 board's warm reset hung one time in
/// three, a lamp left dark after a pinhole reset. A product reset doesn't
/// reboot: after the wipe it starts, in place, what it stopped that a boot
/// starts outside `lampos.target` and was running (not the updater's way
/// back, which its timer starts), and setup, which serves with the marker
/// gone and brings up its hotspot.
fn product_lamp() -> Fake {
    Fake {
        parts: "lamp-voice.service".into(),
        starters: "lamp-skills-manage.socket loaded active listening Skills\nlamp-setup-change.socket loaded inactive dead Change\nlamp-update-back.timer loaded active waiting Back\nlamp-update-back.service loaded active running Back\n".into(),
        writers: "lamp-setup.service loaded inactive dead Setup\n\u{25cf} lamp-update.service loaded active running Update\nlamp-records.service loaded inactive dead Records\n".into(),
        ..Fake::default()
    }
}

#[test]
fn a_product_reset_starts_setup_in_place_instead_of_rebooting() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = product_lamp();
    let steps = reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    assert_eq!(
        &steps[steps.len() - 2..],
        [Step::JournalRemoved, Step::Started]
    );
    let argvs = fake.argvs.borrow();
    assert_eq!(
        argvs.last().unwrap(),
        &[
            "/bin/systemctl",
            "start",
            "lamp-skills-manage.socket",
            "lamp-update-back.timer",
            "lamp-update.service",
            "lamp-setup.service"
        ]
        .map(String::from),
        "what was running and a boot starts, and setup; never a stopped one or the way back"
    );
    assert!(
        !fake
            .calls
            .borrow()
            .iter()
            .any(|call| call == "reboot" || call == "force")
    );
    // Started only after the wipe, the journal's removal, /run emptied of
    // what a reboot would have cleared (the bus's latest, the photos), and
    // the old network and the journal forgotten (#4513's P3s).
    let calls = fake.calls.borrow();
    assert_eq!(
        calls[calls.len() - 8..],
        [
            "sync",
            "sync",
            "clear",
            "-i",
            "-i",
            "--rotate",
            "--vacuum-time=1s",
            "start"
        ]
    );
    let forgotten = argvs[argvs.len() - 5..argvs.len() - 1].to_vec();
    let forget: Vec<Vec<String>> = reset::FORGET
        .iter()
        .map(|argv| argv.iter().map(|word| (*word).to_owned()).collect())
        .collect();
    assert_eq!(forgotten, forget);
}

/// A start in place that fails reboots, then forces one, as before: a reset
/// never leaves the lamp dark by itself.
#[test]
fn a_start_in_place_that_fails_reboots_as_before() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        fail_start: true,
        ..product_lamp()
    };
    let steps = reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    assert_eq!(steps.last(), Some(&Step::Rebooting));
    let calls = fake.calls.borrow();
    assert_eq!(calls[calls.len() - 2..], ["start", "reboot"]);
}

/// Without setup's unit nothing would bring the lamp back, and a keeper's
/// plan (a dev lamp) is the plan's as it is: either way, the reboot.
#[test]
fn a_lamp_without_setups_unit_or_a_keepers_plan_reboots() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        writers: "\u{25cf} lamp-update.service loaded active running Update\n".into(),
        ..product_lamp()
    };
    let steps = reset::reset(&plan, &mut fake).unwrap();
    assert_eq!(steps.last(), Some(&Step::Rebooting));
    assert!(!fake.calls.borrow().iter().any(|call| call == "start"));

    let root = Root::new();
    let mut plan = lamp(&root);
    plan.stop = vec![
        "/opt/lampOS/keeper/current/lamp-keeper".into(),
        "stop".into(),
        "lamp".into(),
    ];
    let mut fake = product_lamp();
    let steps = reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    assert_eq!(steps.last(), Some(&Step::Rebooting));
    assert!(!fake.calls.borrow().iter().any(|call| call == "start"));
}

/// A stop that fails still reboots at once without wiping or starting:
/// something may be running, and setup finishes the wipe at its start.
#[test]
fn a_failed_stop_of_a_product_lamp_reboots_without_starting() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        fail_stop: true,
        ..product_lamp()
    };
    let steps = reset::reset(&plan, &mut fake).unwrap();
    assert_eq!(steps.last(), Some(&Step::Rebooting));
    assert!(
        root.path("setup/reset.json").exists(),
        "the journal is kept"
    );
    assert!(!fake.calls.borrow().iter().any(|call| call == "start"));
}

/// What a reboot cleared in `/run` (the bus's latest messages, the app's
/// photos) must be emptied before setup starts in place: if it can't be,
/// the reset reboots, which clears it, and starts nothing.
#[test]
fn run_not_emptied_reboots_instead_of_starting_in_place() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        fail_clear: true,
        ..product_lamp()
    };
    let steps = reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    assert_eq!(steps.last(), Some(&Step::Rebooting));
    let calls = fake.calls.borrow();
    assert_eq!(calls[calls.len() - 2..], ["clear", "reboot"]);
    assert!(!calls.iter().any(|call| call == "start"));
}

/// What starting in place empties is every folder a part keeps in `/run`
/// past its stop, and only those: each `/run/lampos-*` folder the product's
/// tmpfiles make (but the locks, which hold no one's words), and each
/// product unit's `RuntimeDirectory=` with `RuntimeDirectoryPreserve=yes`.
/// A unit that starts keeping its folder fails this until it is named.
#[test]
fn what_starting_in_place_empties_is_every_folder_kept_in_run_past_a_stop() {
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let mut kept: Vec<String> = fs::read_to_string(product.join("lampos.conf"))
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("d "))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter(|folder| {
            folder.starts_with("/run/lampos-") && !folder.starts_with("/run/lampos-locks")
        })
        .map(String::from)
        .collect();
    for entry in fs::read_dir(&product).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|kind| kind != "service") {
            continue;
        }
        let unit = fs::read_to_string(&path).unwrap();
        if unit
            .lines()
            .any(|line| line.trim() == "RuntimeDirectoryPreserve=yes")
        {
            for line in unit.lines() {
                if let Some(folders) = line.trim().strip_prefix("RuntimeDirectory=") {
                    kept.extend(
                        folders
                            .split_whitespace()
                            .map(|folder| format!("/run/{folder}")),
                    );
                }
            }
        }
    }
    kept.sort();
    let mut named: Vec<String> = reset::VOLATILE
        .iter()
        .map(|folder| (*folder).to_owned())
        .collect();
    named.sort();
    assert_eq!(named, kept);
}

/// The old network and the journal are forgotten best effort: a lamp
/// without `wpa_cli` or `journalctl`, or one that refuses, still starts
/// setup in place, never reboots for it.
#[test]
fn forgetting_the_old_network_or_the_journal_never_stops_the_start() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = Fake {
        fail_forget: true,
        ..product_lamp()
    };
    let steps = reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    assert_eq!(steps.last(), Some(&Step::Started));
    let calls = fake.calls.borrow();
    assert_eq!(calls.last().map(String::as_str), Some("start"));
    assert!(!calls.iter().any(|call| call == "reboot" || call == "force"));
}

#[test]
fn a_forget_that_hangs_is_killed_at_its_bound_and_never_holds_the_start() {
    let words = |argv: &[&str]| {
        argv.iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let started = Instant::now();
    let error =
        reset::run_within(&words(&["sleep", "30"]), Duration::from_millis(200)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    reset::run_within(&words(&["true"]), Duration::from_secs(5)).unwrap();
    assert!(reset::run_within(&words(&["false"]), Duration::from_secs(5)).is_err());
}

/// A reset started in place never reboots (#4500), so a product unit that
/// waits for the setup marker but is not part of `lampos.target` is not
/// stopped with the lamp, nor held off by its condition while setup runs:
/// the reset must stop it ([`reset::WRITERS`]) or start it again
/// ([`reset::STARTERS`]). Only the boot's own units, which a start in place
/// does not run, are left: the waking face, which every voice start shows
/// too. On an office lamp `lamp-peek` was such a unit and kept port 80 from
/// setup (`peek.md`); a product unit like it fails this until it is named.
#[test]
fn every_product_unit_outside_the_target_that_waits_for_setup_is_the_resets() {
    const BOOT_ONLY: [&str; 1] = ["lamp-waking.service"];
    let named = |unit: &str| {
        reset::STARTERS
            .iter()
            .chain(&reset::WRITERS)
            .any(|pattern| match pattern.split_once('*') {
                Some((head, tail)) => {
                    unit.starts_with(head)
                        && unit.ends_with(tail)
                        && unit.len() >= head.len() + tail.len()
                }
                None => *pattern == unit,
            })
    };
    let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let mut outside = Vec::new();
    for entry in fs::read_dir(&product).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let kind = path
            .extension()
            .map(|kind| kind.to_string_lossy().into_owned());
        if !matches!(
            kind.as_deref(),
            Some("service" | "socket" | "timer" | "path")
        ) || name == "lampos.target"
        {
            continue;
        }
        let unit = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = unit.lines().map(str::trim).collect();
        if lines.contains(&"ConditionPathExists=/var/lib/lampos-setup/done")
            && !lines.contains(&"PartOf=lampos.target")
        {
            outside.push(name);
        }
    }
    outside.sort();
    assert!(!outside.is_empty(), "the product's units were read");
    let unnamed: Vec<&String> = outside
        .iter()
        .filter(|unit| !named(unit) && !BOOT_ONLY.contains(&unit.as_str()))
        .collect();
    assert!(
        unnamed.is_empty(),
        "outside lampos.target, waiting for setup, and neither stopped nor started by a reset in place: {unnamed:?}"
    );
}

/// An office lamp's `lamp-peek` on port 80 (#4536): a reset in place stops
/// it with the lamp, as a writer, and never starts it again, so setup's page
/// has the port; the lamp's next boot starts it once setup is done.
#[test]
fn a_reset_in_place_stops_lamp_peek_and_leaves_it_off_for_setup() {
    let root = Root::new();
    let plan = lamp(&root);
    let mut fake = product_lamp();
    fake.writers
        .push_str("lamp-peek.service loaded active running Peek\n");
    let steps = reset::reset(&plan, &mut fake).unwrap();
    wiped(&root);
    assert_eq!(steps.last(), Some(&Step::Started));
    let argvs = fake.argvs.borrow();
    let stop = argvs
        .iter()
        .find(|argv| argv[1] == "stop" && argv.iter().any(|word| word == "lampos.target"))
        .unwrap();
    assert!(
        stop.iter().any(|unit| unit == "lamp-peek.service"),
        "{stop:?}"
    );
    let start = argvs.iter().rfind(|argv| argv[1] == "start").unwrap();
    assert!(
        !start.iter().any(|unit| unit == "lamp-peek.service"),
        "{start:?}"
    );
    assert!(
        start.iter().any(|unit| unit == "lamp-setup.service"),
        "{start:?}"
    );
}
