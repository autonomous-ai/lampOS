//! A product unit the lamp lacks, added by a round (#3689 item 2b): with
//! its part when the part is new, enabled as `install` enables it, started
//! when what enables it runs, and taken away again with a build put back.
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

fn binaries(mood: &str) -> Vec<(&'static str, String)> {
    vec![
        ("lamp-eyes", "eyes 1".into()),
        ("lamp-mood", mood.into()),
        ("lamp-records", "records 1".into()),
    ]
}

fn publish_all(lamp: &mut Lamp, level: u64, mood: &str) {
    let binaries = binaries(mood);
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

/// What a lamp had before, without the part folders a put-back leaves
/// empty.
fn without_part_folders(files: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    files
        .into_iter()
        .filter(|(name, _)| !["opt/lampOS/mood/", "opt/lampOS/records/"].contains(&name.as_str()))
        .collect()
}

#[test]
fn a_new_part_its_timer_and_a_helper_of_a_part_the_lamp_has_are_added_enabled_and_started() {
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
    assert_eq!(
        lamp.calls(),
        [
            "reload",
            "restart lamp-eyes-camera.socket",
            "restart lamp-mood.service",
            "restart lamp-records.timer",
        ]
    );
}

#[test]
fn an_added_unit_waits_for_what_enables_it_when_that_is_not_running() {
    let mut lamp = product_lamp();
    // Setup not done: lampos.target does not run.
    lamp.systemd.units.remove("lampos.target");
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1");
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(
        link(
            &lamp,
            &format!("/etc/systemd/system/lampos.target.wants/{MOOD}")
        )
        .is_some()
    );
    assert!(!lamp.calls().contains(&format!("restart {MOOD}")));
}

#[test]
fn a_build_put_back_takes_away_every_unit_link_and_part_it_added() {
    let mut lamp = product_lamp();
    let before = without_part_folders(lamp.lamp_files());
    publish_all(&mut lamp, manifest::ADDS_UPDATER, "mood 1 which crashes");
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
    assert_eq!(without_part_folders(lamp.lamp_files()), before);
    for unit in [SOCKET, MOOD, RECORDS_TIMER] {
        assert!(lamp.calls().contains(&format!("stop {unit}")), "{unit}");
    }
}

#[test]
fn a_switch_cut_short_is_put_back_and_takes_away_what_it_added() {
    let mut lamp = product_lamp();
    let before = without_part_folders(lamp.lamp_files());
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
    assert_eq!(without_part_folders(lamp.lamp_files()), before);
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
