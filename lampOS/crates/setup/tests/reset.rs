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

#[derive(Default)]
struct Fake {
    calls: Rc<RefCell<Vec<String>>>,
    fail_stop: bool,
    fail_reboot: bool,
}
impl Effects for Fake {
    fn run(&mut self, argv: &[String]) -> io::Result<()> {
        self.calls.borrow_mut().push(argv[1].clone());
        if (self.fail_stop && argv[1] == "stop") || (self.fail_reboot && argv[1] == "reboot") {
            return Err(io::Error::other("refused"));
        }
        Ok(())
    }
    fn sync(&mut self) {
        self.calls.borrow_mut().push("sync".into());
    }
    fn force_reboot(&mut self) {
        self.calls.borrow_mut().push("force".into());
    }
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
        ["sync", "stop", "sync", "sync", "reboot"]
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
    assert_eq!(*fake.calls.borrow(), ["sync", "stop", "reboot"]);
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
const PERSON_STORES: [&str; 27] = [
    // lamp-4ace's units.
    "/var/lib/lampos-memory",
    "/var/lib/lampos-eyes/people.json",
    // Face prints the eyes could not read and set aside, never dropped
    // (#1505): a factory reset must not leave them behind.
    "/var/lib/lampos-eyes/set-aside",
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
    "/var/lib/lampos-output-caps",
    "/var/lib/lampos-policy",
    "/var/lib/lampOS/voice/sessions",
    "/var/lib/lampOS/voice/schedule-delivery",
    "/var/lib/lampOS/voice/first-output",
    "/run/lampos-voice/thread.json",
    // The voice's default for the task news it has told (reviewer, #817):
    // `DEFAULT_TOLD` in crates/voice/src/task_news.rs.
    "/var/lib/lampOS/voice/tasks-told",
    // The person's light the ring keeps with `LAMP_LIGHT_KEEP=on` (#1468):
    // the folder, so a `person.json.new` a power cut left goes too.
    "/var/lib/lampOS/light",
    // What setup saves, and its marker: the user's key, to the voice and to
    // the Policy (the owner, #806, 16:13), and the name.
    "/etc/lampOS/voice/credential",
    "/etc/lampOS/policy/jev.key",
    "/etc/lampOS/person/name",
    "/etc/wpa_supplicant/wpa_supplicant-wlan0.conf",
    "/var/lib/lampos-setup/done",
];

#[test]
fn the_example_plan_wipes_every_store_of_the_person() {
    let plan: Plan =
        serde_json::from_str(include_str!("../fixtures/reset-plan-example.json")).unwrap();
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

/// Harness's real layout under the tasks part's home (Nine on #1016, from
/// installing it on lamp-75c9): the product plan's Harness paths, run by
/// the real wipe, take the previous owner's sign-in, pairings and logs and
/// leave Harness itself (its CLI, its own Node, its wrapper) able to run.
#[test]
fn a_reset_takes_harness_sign_in_and_pairings_and_keeps_harness_itself() {
    let plan: Plan =
        serde_json::from_str(include_str!("../fixtures/reset-plan-product.json")).unwrap();
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

/// A lamp put on by `lamp-update install` has no keeper (install.md, "Setup
/// at first boot, without the keeper"): its plan stops the lamp with
/// systemd, and is the example's in everything else, so the two never drift.
#[test]
fn the_product_plan_is_the_example_stopped_by_systemd() {
    let example: Plan =
        serde_json::from_str(include_str!("../fixtures/reset-plan-example.json")).unwrap();
    let product: Plan =
        serde_json::from_str(include_str!("../fixtures/reset-plan-product.json")).unwrap();
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
        serde_json::from_str(include_str!("../fixtures/reset-plan-product.json")).unwrap();
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
    // The subscriber is found by the publisher's next discovery.
    std::thread::sleep(Duration::from_millis(250));
    for event in &events {
        publisher.publish_json(event.stamp.at_us, event).unwrap();
    }
    let acted = RefCell::new(0);
    let logged = RefCell::new(Vec::new());
    let now = events.last().map_or(0, |event| event.stamp.at_us + 1_000);
    let watched = lamp_setup::watch(
        &mut touch,
        CLOCK,
        enabled,
        Some(Instant::now() + Duration::from_millis(600)),
        || now,
        |line| logged.borrow_mut().push(line.to_owned()),
        || {
            *acted.borrow_mut() += 1;
            Ok(())
        },
    )
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
