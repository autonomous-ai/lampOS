//! A product unit the lamp lacks, added by a round (#3689 item 2b): with
//! its part when the part is new, enabled as `install` enables it, never
//! started inside the watch, a long-running part of `lampos.target` started
//! once the watch passed, and taken away again with a build put back.
use super::*;

const MOOD: &str = "lamp-mood.service";
const RECORDS: &str = "lamp-records.service";
const RECORDS_TIMER: &str = "lamp-records.timer";
const TEMPLATE: &str = "lamp-eyes-camera@.service";
const SOCKET: &str = "lamp-eyes-camera.socket";
const KEYS: &str = "lamp-setup-keys.socket";

fn mood() -> String {
    "[Service]\nUser=lampos\nGroup=lampos\nExecStart=/opt/lampOS/mood/current/lamp-mood\n[Install]\nWantedBy=lampos.target\n".into()
}

fn records() -> String {
    "[Service]\nExecStart=/opt/lampOS/records/current/lamp-records check\n".into()
}

fn records_timer() -> String {
    "[Timer]\nOnCalendar=hourly\n[Install]\nWantedBy=timers.target\n".into()
}

fn template() -> String {
    "[Service]\nExecStart=/opt/lampOS/eyes/current/lamp-eyes reauthorize-camera\n".into()
}

fn socket() -> String {
    "[Socket]\nListenStream=/run/lampos-eyes-camera/camera.sock\nAccept=yes\n[Install]\nWantedBy=sockets.target\n".into()
}

/// A lamp installed as the product: its eyes started through their link,
/// no unit of its own, a managed updater, its users, and setup done (so
/// `lampos.target` runs).
fn product_lamp() -> Lamp {
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    for unit in [
        "lamp-voice.service",
        "lamp-body.service",
        "lamp-controls.service",
    ] {
        fs::remove_file(lamp.root.join("etc/systemd/system").join(unit)).unwrap();
    }
    let hash = sha("update 1");
    let folder = lamp.root.join(format!("opt/lampOS/update/{hash}"));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("lamp-update"), "update 1").unwrap();
    std::os::unix::fs::symlink(hash, lamp.root.join("opt/lampOS/update/current")).unwrap();
    fs::write(
        lamp.root.join("etc/systemd/system/lamp-update.service"),
        "[Service]\nExecStart=/opt/lampOS/update/current/lamp-update serve\n",
    )
    .unwrap();
    fs::write(
        lamp.root.join("etc/passwd"),
        "root:x:0:0::/root:/bin/sh\nlampos:x:999:999::/nonexistent:/usr/sbin/nologin\n",
    )
    .unwrap();
    fs::write(lamp.root.join("etc/group"), "root:x:0:\nlampos:x:999:\n").unwrap();
    for target in ["lampos.target", "timers.target", "sockets.target"] {
        lamp.systemd.units.insert(
            target.into(),
            Running {
                active: "active".into(),
                sub: "active".into(),
                ..Running::default()
            },
        );
    }
    lamp
}

/// Build 2 at updater `level`: these binaries, and these product units.
fn publish(lamp: &mut Lamp, level: u64, binaries: &[(&str, &str)], units: &[(&str, String)]) {
    let mut build = lamp.manifest(2, binaries, &[]);
    for (name, text) in units {
        let unit = Unit {
            lamp: manifest::PRODUCT_LAMP.into(),
            unit: manifest::manifest_name(name),
            sha256: sha(text),
            size: text.len() as u64,
        };
        lamp.channel.files.insert(
            format!("{}{}", build.files_url, unit.file_name()),
            text.as_bytes().to_vec(),
        );
        build.units.push(unit);
    }
    build.min_updater = Some(level);
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        manifest::seal(&serde_json::to_vec(&build).unwrap(), &lamp.key).unwrap(),
    );
}

fn all() -> Vec<(&'static str, String)> {
    vec![
        (MOOD, mood()),
        (RECORDS, records()),
        (RECORDS_TIMER, records_timer()),
        (TEMPLATE, template()),
        (SOCKET, socket()),
        ("lamp-eyes.service", eyes_unit()),
    ]
}

fn binaries(eyes: &str, mood: &str) -> Vec<(&'static str, String)> {
    vec![
        ("lamp-eyes", eyes.into()),
        ("lamp-mood", mood.into()),
        ("lamp-records", "records 1".into()),
    ]
}

fn publish_all(lamp: &mut Lamp, level: u64, mood: &str) {
    publish_with_eyes(lamp, level, "eyes 1", mood);
}

fn publish_with_eyes(lamp: &mut Lamp, level: u64, eyes: &str, mood: &str) {
    let binaries = binaries(eyes, mood);
    let binaries: Vec<(&str, &str)> = binaries
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    publish(lamp, level, &binaries, &all());
}

fn link(lamp: &Lamp, path: &str) -> Option<String> {
    fs::read_link(lamp.root.join(path.trim_start_matches('/')))
        .ok()
        .map(|to| to.display().to_string())
}

#[test]
fn a_new_part_its_timer_and_a_helper_of_a_part_the_lamp_has_are_added_and_enabled_and_the_part_started()
 {
    let mut lamp = product_lamp();
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    for (name, text) in all().into_iter().take(5) {
        assert_eq!(
            lamp.read(&format!("/etc/systemd/system/{name}")),
            text,
            "{name}"
        );
    }
    // New parts: installed afresh, each through its link.
    assert_eq!(link(&lamp, "/opt/lampOS/mood/current"), Some(sha("mood 1")));
    assert_eq!(
        link(&lamp, "/opt/lampOS/records/current"),
        Some(sha("records 1"))
    );
    assert_eq!(
        lamp.read(&format!("/opt/lampOS/mood/{}/lamp-mood", sha("mood 1"))),
        "mood 1"
    );
    // Enabled as install enables them; the template and the timed service
    // never.
    for (wants, unit) in [
        ("lampos.target.wants", MOOD),
        ("timers.target.wants", RECORDS_TIMER),
        ("sockets.target.wants", SOCKET),
    ] {
        assert_eq!(
            link(&lamp, &format!("/etc/systemd/system/{wants}/{unit}")),
            Some(format!("/etc/systemd/system/{unit}")),
            "{unit}"
        );
    }
    assert!(!lamp.exists("/etc/systemd/system/multi-user.target.wants"));
    // The new part of lampos.target is started once the watch passed, not
    // left for the next start; the timer, the socket, the timed service and
    // the template are, as install leaves them.
    assert_eq!(
        lamp.calls(),
        ["reload", &format!("start --no-block {MOOD}")]
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line == &format!("started {MOOD}, added by build 2")),
        "{:?}",
        lamp.said
    );
    // A round with nothing to switch starts nothing.
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(lamp.calls().len(), 2);
}

/// A part added and enabled that already runs is never restarted, and one
/// that is not enabled, or a lamp whose `lampos.target` does not run (not
/// set up), starts none.
#[test]
fn an_added_part_already_running_disabled_or_on_a_lamp_not_running_is_not_started() {
    // Running already (started by something else before the watch ended).
    let mut lamp = product_lamp();
    lamp.systemd.units.insert(
        MOOD.into(),
        Running {
            active: "active".into(),
            sub: "running".into(),
            pid: 7,
            ..Running::default()
        },
    );
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    // Restarted and watched as any running part; never started again.
    assert_eq!(lamp.calls(), ["reload", &format!("restart {MOOD}")]);

    // lampos.target not running: the part starts with it.
    let mut lamp = product_lamp();
    lamp.systemd.units.remove("lampos.target");
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.calls(), ["reload"]);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("start with lampos.target, which is not running")),
        "{:?}",
        lamp.said
    );

    // Its [Install] names no lampos.target: never enabled into the lamp.
    let mut lamp = product_lamp();
    let binaries = binaries("eyes 1", "mood 1");
    let binaries: Vec<(&str, &str)> = binaries
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    let unwanted = mood().replace("WantedBy=lampos.target", "WantedBy=sysinit.target");
    publish(
        &mut lamp,
        manifest::ADDS_UPDATER,
        &binaries,
        &[(MOOD, unwanted)],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.calls(), ["reload"]);
}

/// A start systemd refuses is said, and the build is kept.
#[test]
fn an_added_part_whose_start_fails_is_said_and_the_round_stays_switched() {
    let mut lamp = product_lamp();
    lamp.systemd.start_refused = Some("Unit lamp-mood.service is masked".into());
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(
        lamp.calls(),
        ["reload", &format!("start --no-block {MOOD}")]
    );
    assert!(
        lamp.said.iter().any(|line| line
            == &format!(
                "{MOOD} could not be started (Unit lamp-mood.service is masked); it starts at the next boot"
            )),
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
}

#[test]
fn an_added_unit_that_ends_at_once_holds_nothing_back_since_it_is_started_after_the_watch() {
    // The waking face, the Wi-Fi restore and recovery end at once when they
    // have nothing to do; one started and watched would read as stopped.
    let mut lamp = product_lamp();
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1 which crashes");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
    assert!(
        link(
            &lamp,
            &format!("/etc/systemd/system/lampos.target.wants/{MOOD}")
        )
        .is_some()
    );
    // Started only once the watch passed and the build was kept.
    assert_eq!(
        lamp.calls(),
        ["reload", &format!("start --no-block {MOOD}")]
    );
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
}

#[test]
fn a_build_put_back_takes_away_every_unit_link_and_part_it_added() {
    let mut lamp = product_lamp();
    let before = lamp.lamp_files();
    // The eyes' new build is not well: everything goes back.
    publish_with_eyes(
        &mut lamp,
        manifest::ADDS_UPDATER,
        "eyes 2 which crashes",
        "mood 1",
    );
    assert!(
        matches!(
            lamp.round(),
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.lamp_files(), before);
    for unit in [SOCKET, MOOD, RECORDS_TIMER] {
        assert!(lamp.calls().contains(&format!("stop {unit}")), "{unit}");
    }
}

#[test]
fn an_enabling_link_already_there_is_left_as_it_is_both_ways() {
    let mut lamp = product_lamp();
    let wants = lamp.root.join("etc/systemd/system/timers.target.wants");
    fs::create_dir_all(&wants).unwrap();
    std::os::unix::fs::symlink(
        "/etc/systemd/system/lamp-records.timer",
        wants.join(RECORDS_TIMER),
    )
    .unwrap();
    publish_with_eyes(
        &mut lamp,
        manifest::ADDS_UPDATER,
        "eyes 2 which crashes",
        "mood 1",
    );
    assert!(
        matches!(lamp.round(), Outcome::RolledBack { sequence: 2, .. }),
        "{:?}",
        lamp.said
    );
    assert_eq!(
        link(
            &lamp,
            &format!("/etc/systemd/system/timers.target.wants/{RECORDS_TIMER}")
        ),
        Some(format!("/etc/systemd/system/{RECORDS_TIMER}"))
    );
    assert!(!lamp.exists(&format!("/etc/systemd/system/{RECORDS_TIMER}")));
}

#[test]
fn a_switch_cut_short_is_put_back_and_takes_away_what_it_added() {
    let mut lamp = product_lamp();
    let before = lamp.lamp_files();
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err());
    assert!(
        lamp.exists(&format!("/etc/systemd/system/{MOOD}")),
        "written before the cut"
    );
    assert!(lamp.exists("/var/lib/lampos-update/journal.json"));
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
        "{outcome:?}"
    );
    assert_eq!(lamp.lamp_files(), before);
}

#[test]
fn a_unit_that_runs_as_a_user_the_lamp_lacks_is_not_added_and_that_is_said() {
    let mut lamp = product_lamp();
    fs::write(lamp.root.join("etc/passwd"), "root:x:0:0::/root:/bin/sh\n").unwrap();
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(!lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
    assert!(!lamp.exists("/opt/lampOS/mood"));
    assert!(lamp.exists(&format!("/etc/systemd/system/{RECORDS_TIMER}")));
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("lamp-mood.service is not added")
                && line.contains("user lampos")),
        "{:?}",
        lamp.said
    );
}

#[test]
fn a_lamp_of_its_own_units_an_older_build_or_a_unit_install_leaves_out_gets_nothing_added() {
    // lamp-4ace: its voice, body and controls are its own pinned units.
    let mut lamp = Lamp::new().with_linked_eyes("eyes 1");
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    lamp.round();
    assert!(!lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
    // A build below the level that adds.
    let mut lamp = product_lamp();
    publish_all(&mut lamp, manifest::COMPANIONS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(!lamp.exists(&format!("/etc/systemd/system/{MOOD}")));
    // The own keys' helper, left out of install until its lamp check.
    let mut lamp = product_lamp();
    let keys = "[Socket]\nListenStream=/run/lampos-setup-keys/keys.sock\nAccept=yes\n[Install]\nWantedBy=lampos.target\n";
    publish(
        &mut lamp,
        manifest::ADDS_UPDATER,
        &[("lamp-eyes", "eyes 1")],
        &[(KEYS, keys.into())],
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(!lamp.exists(&format!("/etc/systemd/system/{KEYS}")));
    assert!(!lamp.channel.fetched.iter().any(|url| url.ends_with(KEYS)));
}

/// A lamp already given lamp-mood at build `text`, enabled into
/// `lampos.target`, whose last run is in `running`.
fn with_mood(mut lamp: Lamp, text: &str, running: Running) -> Lamp {
    let build = sha(text);
    let folder = lamp.root.join(format!("opt/lampOS/mood/{build}"));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("lamp-mood"), text).unwrap();
    std::os::unix::fs::symlink(&build, lamp.root.join("opt/lampOS/mood/current")).unwrap();
    fs::write(lamp.root.join("etc/systemd/system").join(MOOD), mood()).unwrap();
    let wants = lamp.root.join("etc/systemd/system/lampos.target.wants");
    fs::create_dir_all(&wants).unwrap();
    std::os::unix::fs::symlink(format!("/etc/systemd/system/{MOOD}"), wants.join(MOOD)).unwrap();
    lamp.systemd.units.insert(MOOD.into(), running);
    lamp
}

fn ended(result: &str) -> Running {
    Running {
        active: if result == "success" {
            "inactive"
        } else {
            "failed"
        }
        .into(),
        sub: if result == "success" {
            "dead"
        } else {
            "failed"
        }
        .into(),
        result: result.into(),
        ..Running::default()
    }
}

/// #6699: lamp-control exited 0 under a build where its switch was off, and
/// three updates that changed it left it dead until a manual restart. A
/// part the lamp had, that the switch changed and that ended cleanly, is
/// started once the watch passed, as an added part is.
#[test]
fn a_changed_part_that_ended_cleanly_under_the_old_build_is_started_after_the_switch() {
    let mut lamp = with_mood(product_lamp(), "mood 1", ended("success"));
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 2");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(link(&lamp, "/opt/lampOS/mood/current"), Some(sha("mood 2")));
    // Not restarted inside the watch (it was not running), started after.
    assert_eq!(
        lamp.calls(),
        ["reload", &format!("start --no-block {MOOD}")]
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line == &format!("started {MOOD}, ended before build 2, which changed it")),
        "{:?}",
        lamp.said
    );
}

/// A failed part is left as an added one is; one the build did not change
/// is not this round's; on a lamp whose `lampos.target` does not run none
/// is started.
#[test]
fn a_changed_part_failed_unchanged_or_on_a_lamp_not_running_is_not_started() {
    // Failed under the old build: left for the person, as now.
    let mut lamp = with_mood(product_lamp(), "mood 1", ended("exit-code"));
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 2");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(
        !lamp.calls().iter().any(|call| call.contains(MOOD)),
        "{:?}",
        lamp.calls()
    );

    // Not changed by the build (the same binary and unit): not started.
    let mut lamp = with_mood(product_lamp(), "mood 1", ended("success"));
    publish_with_eyes(&mut lamp, manifest::ADDS_UPDATER, "eyes 2", "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(
        !lamp.calls().iter().any(|call| call.contains(MOOD)),
        "{:?}",
        lamp.calls()
    );

    // lampos.target not running: the part starts with it.
    let mut lamp = with_mood(product_lamp(), "mood 1", ended("success"));
    lamp.systemd.units.remove("lampos.target");
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 2");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(
        !lamp.calls().iter().any(|call| call.contains(MOOD)),
        "{:?}",
        lamp.calls()
    );
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("start with lampos.target, which is not running")),
        "{:?}",
        lamp.said
    );
}

const CONTROL: &str = "lamp-control.service";

/// lamp-control's unit as the product enables it, `switch` in its own
/// environment (none for `""`), its last run in `running`.
fn with_control(mut lamp: Lamp, switch: &str, running: Running) -> Lamp {
    let environment = if switch.is_empty() {
        String::new()
    } else {
        format!("Environment={switch}\n")
    };
    let build = sha("control 1");
    let folder = lamp.root.join(format!("opt/lampOS/control/{build}"));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("lamp-control"), "control 1").unwrap();
    std::os::unix::fs::symlink(&build, lamp.root.join("opt/lampOS/control/current")).unwrap();
    fs::write(
        lamp.root.join("etc/systemd/system").join(CONTROL),
        format!(
            "[Service]\nUser=lampos\n{environment}ExecStart=/opt/lampOS/control/current/lamp-control serve\n[Install]\nWantedBy=lampos.target\n"
        ),
    )
    .unwrap();
    let wants = lamp.root.join("etc/systemd/system/lampos.target.wants");
    fs::create_dir_all(&wants).unwrap();
    std::os::unix::fs::symlink(
        format!("/etc/systemd/system/{CONTROL}"),
        wants.join(CONTROL),
    )
    .unwrap();
    lamp.systemd.units.insert(CONTROL.into(), running);
    lamp
}

fn control_starts(lamp: &Lamp) -> usize {
    lamp.calls()
        .iter()
        .filter(|call| *call == &format!("start --no-block {CONTROL}"))
        .count()
}

/// #6699, V2's case: a lamp-control that ended cleanly under an older build
/// while a switch that keeps it up is on is started by a kept switch even
/// when the build did not change it, and by an up-to-date round: the round
/// that installs a build is run by the updater the lamp had, so the new
/// updater's first round finds the lamp up to date.
#[test]
fn lamp_control_ended_cleanly_with_its_switch_on_is_started_by_a_switch_or_an_up_to_date_round() {
    let mut lamp = with_control(product_lamp(), "LAMP_SKILL_TASKS=on", ended("success"));
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(control_starts(&lamp), 1, "{:?}", lamp.calls());
    assert!(
        lamp.said
            .iter()
            .any(|line| line
                == &format!("started {CONTROL}, ended cleanly with LAMP_SKILL_TASKS on")),
        "{:?}",
        lamp.said
    );

    // Running: an up-to-date round starts nothing.
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(control_starts(&lamp), 1, "{:?}", lamp.calls());

    // Ended cleanly again: the next up-to-date round starts it.
    lamp.systemd.units.insert(CONTROL.into(), ended("success"));
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(control_starts(&lamp), 2, "{:?}", lamp.calls());

    // The developers' door through a drop-in keeps it up as well.
    let drop_in = lamp.root.join(format!("etc/systemd/system/{CONTROL}.d"));
    fs::create_dir_all(&drop_in).unwrap();
    fs::write(
        drop_in.join("off.conf"),
        "[Service]\nEnvironment=LAMP_SKILL_TASKS=off\nEnvironment=LAMP_CONTROL=on\n",
    )
    .unwrap();
    lamp.systemd.units.insert(CONTROL.into(), ended("success"));
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(control_starts(&lamp), 3, "{:?}", lamp.calls());
    assert!(
        lamp.said
            .iter()
            .any(|line| line == &format!("started {CONTROL}, ended cleanly with LAMP_CONTROL on")),
        "{:?}",
        lamp.said
    );
}

/// Off by both switches it exits 0 by design and is left; a failed one is
/// left as any failed part is; one its unit's condition skipped (no setup
/// marker) is left too, or every round would start it again (Twelve's P3
/// on #6708).
#[test]
fn lamp_control_off_by_both_switches_or_failed_is_not_started() {
    let skipped = Running {
        condition: "no".into(),
        ..ended("success")
    };
    let mut lamp = with_control(product_lamp(), "LAMP_SKILL_TASKS=on", skipped);
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(control_starts(&lamp), 0, "{:?}", lamp.calls());

    let mut lamp = with_control(product_lamp(), "", ended("success"));
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(control_starts(&lamp), 0, "{:?}", lamp.calls());

    let mut lamp = with_control(product_lamp(), "LAMP_SKILL_TASKS=on", ended("exit-code"));
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(control_starts(&lamp), 0, "{:?}", lamp.calls());
}

/// A unit of lamp-update's own that a build adds (the region's, #6386)
/// starts through `update/current`, which the lamp has and no part owns:
/// it is added beside lamp-update's own folder, and the link is left as it
/// is. Refused, every later build was refused on a lamp that took the
/// region's build through a round (lamp-52e6, 12635 to 12661).
#[test]
#[ignore = "PARKED (#6386): lamp-region.service is out of install::PRODUCT, so a round adds no unit of lamp-update's own; re-enable when China is scheduled and stable's lamp-update carries #6810"]
fn a_unit_of_lamp_updates_own_added_by_a_build_starts_through_its_current_link_and_switches() {
    let mut lamp = product_lamp();
    let region =
        "[Service]\nType=oneshot\nExecStart=/opt/lampOS/update/current/lamp-update region\n"
            .to_string();
    let mut units = all();
    units.push(("lamp-region.service", region.clone()));
    let binaries = binaries("eyes 1", "mood 1");
    let mut binaries: Vec<(&str, &str)> = binaries
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    binaries.push(("lamp-update", "update 1"));
    publish(&mut lamp, manifest::ADDS_UPDATER, &binaries, &units);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.read("/etc/systemd/system/lamp-region.service"), region);
    assert_eq!(
        link(&lamp, "/opt/lampOS/update/current"),
        Some(sha("update 1"))
    );
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.contains("no unit of this lamp starts through it")),
        "{:?}",
        lamp.said
    );
}

/// Put back, a build that added a unit of lamp-update's own takes the unit
/// away and leaves lamp-update's folder and link exactly as they were.
#[test]
#[ignore = "PARKED (#6386): lamp-region.service is out of install::PRODUCT, so a round adds no unit of lamp-update's own; re-enable when China is scheduled and stable's lamp-update carries #6810"]
fn a_build_put_back_takes_away_a_unit_of_lamp_updates_own_and_leaves_its_folder() {
    let mut lamp = product_lamp();
    let before = lamp.lamp_files();
    let region =
        "[Service]\nType=oneshot\nExecStart=/opt/lampOS/update/current/lamp-update region\n"
            .to_string();
    let mut units = all();
    units.push(("lamp-region.service", region));
    let binaries = binaries("eyes 2 which crashes", "mood 1");
    let mut binaries: Vec<(&str, &str)> = binaries
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    binaries.push(("lamp-update", "update 1"));
    publish(&mut lamp, manifest::ADDS_UPDATER, &binaries, &units);
    assert!(
        matches!(
            lamp.round(),
            Outcome::RolledBack {
                sequence: 2,
                after: Ok(()),
                ..
            }
        ),
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.lamp_files(), before);
    assert_eq!(
        lamp.read(&format!(
            "/opt/lampOS/update/{}/lamp-update",
            sha("update 1")
        )),
        "update 1"
    );
}
