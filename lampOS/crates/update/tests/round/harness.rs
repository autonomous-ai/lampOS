//! A build that turns Harness on (#3693): Harness's own installer run while
//! the build is fetched, its link and Harness at boot at the switch, and the
//! tasks part started and watched with the rest; all undone on a put-back.
use super::*;

const TASKS: &str = "lamp-tasks.service";
const DONE: &str = "/var/lib/lampos-update/harness-at-boot";

/// lamp-4ace with the tasks part installed and not running: its unit has
/// `ConditionPathExists=/usr/local/bin/harness`, and there is no Harness.
/// Its binary is `tasks` (`crash` in it: it never stays up).
fn tasks_lamp(tasks: &str) -> Lamp {
    let lamp = Lamp::new();
    let build = sha(tasks);
    let folder = lamp.root.join(format!("opt/lampOS/tasks/{build}"));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("lamp-tasks"), tasks).unwrap();
    fs::write(
        lamp.root.join("etc/systemd/system").join(TASKS),
        format!(
            "[Unit]\nConditionPathExists=/usr/local/bin/harness\n[Service]\nExecStart=/opt/lampOS/tasks/{build}/lamp-tasks serve\n"
        ),
    )
    .unwrap();
    lamp
}

/// Build 2, a new voice, published with `harness` and `min_updater`.
fn publish(lamp: &mut Lamp, harness: bool, min_updater: Option<u64>) {
    publish_with(lamp, harness, min_updater, &[]);
}

/// The same, with these binaries too.
fn publish_with(lamp: &mut Lamp, harness: bool, min_updater: Option<u64>, also: &[(&str, &str)]) {
    let mut binaries = vec![
        ("lamp-voice", "voice 2"),
        ("lamp-turn-end", "turn-end 2"),
        ("lamp-body", "body 1"),
        ("lamp-controls", "controls 1"),
    ];
    binaries.extend_from_slice(also);
    let mut build = lamp.manifest(2, &binaries, &[]);
    build.harness = harness;
    build.min_updater = min_updater;
    lamp.channel.files.insert(
        format!("{CHANNEL}{}", manifest::ENVELOPE),
        sealed_unchecked(&serde_json::to_vec(&build).unwrap(), &lamp.key),
    );
}

/// The envelope for these manifest bytes, signed without the publisher's
/// check: what a publisher with the key but not this program could sign.
fn sealed_unchecked(bytes: &[u8], key: &Ed25519KeyPair) -> Vec<u8> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let public: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
    let mut envelope = serde_json::to_vec(&serde_json::json!({
        "format": manifest::FORMAT,
        "key": manifest::fingerprint(&public),
        "manifest": STANDARD.encode(bytes),
        "signature": STANDARD.encode(key.sign(bytes).as_ref()),
    }))
    .unwrap();
    envelope.push(b'\n');
    envelope
}

fn tasks_calls(lamp: &Lamp) -> Vec<String> {
    lamp.calls()
        .into_iter()
        .filter(|call| call.starts_with("harness ") || call.ends_with(TASKS))
        .collect()
}

#[test]
fn a_build_without_harness_never_asks_harness_anything() {
    let mut lamp = tasks_lamp("tasks 1");
    publish(&mut lamp, false, None);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(lamp.harness.asked.is_empty());
    assert!(tasks_calls(&lamp).is_empty(), "{:?}", lamp.calls());
    assert!(!lamp.exists(DONE));
}

#[test]
fn a_build_that_turns_harness_on_fetches_it_links_it_at_the_switch_and_watches_the_tasks_part() {
    let mut lamp = tasks_lamp("tasks 1");
    publish(&mut lamp, true, Some(manifest::HARNESS_UPDATER));
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.harness.asked, ["fetch", "link"]);
    let calls = lamp.calls();
    let at = |call: &str| calls.iter().position(|made| made == call).unwrap();
    // Fetched before anything is written, linked after the units are, and
    // the tasks part started after the link.
    assert!(at("harness fetch") < at("reload"), "{calls:?}");
    assert!(at("reload") < at("harness link"), "{calls:?}");
    assert!(
        at("harness link") < at(&format!("restart {TASKS}")),
        "{calls:?}"
    );
    assert_eq!(lamp.read(DONE), "lampos /var/lib/lampos-tasks\n");
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("Harness is linked")),
        "{:?}",
        lamp.said
    );
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    assert!(!calls.contains(&format!("stop {TASKS}")), "{calls:?}");
}

/// A build that also changes the tasks part, which was not running with no
/// Harness: started on the new build once Harness is linked, and stopped
/// again, on its old unit, when the build is put back.
#[test]
fn a_tasks_part_the_build_changes_is_started_once_harness_is_linked() {
    let mut lamp = tasks_lamp("tasks 1");
    publish_with(
        &mut lamp,
        true,
        Some(manifest::HARNESS_UPDATER),
        &[("lamp-tasks", "tasks 2")],
    );
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.harness.asked, ["fetch", "link"]);
    assert!(
        lamp.read(&format!("/etc/systemd/system/{TASKS}"))
            .contains(&sha("tasks 2"))
    );
    let restarts = lamp
        .calls()
        .into_iter()
        .filter(|call| *call == format!("restart {TASKS}"))
        .count();
    assert_eq!(restarts, 1, "{:?}", lamp.calls());

    let mut lamp = tasks_lamp("tasks 1");
    publish_with(
        &mut lamp,
        true,
        Some(manifest::HARNESS_UPDATER),
        &[("lamp-tasks", "tasks 2 crash")],
    );
    match lamp.round() {
        Outcome::RolledBack { sequence: 2, .. } => {}
        other => panic!("{other:?}: {:?}", lamp.said),
    }
    assert_eq!(lamp.harness.asked, ["fetch", "link", "off"]);
    assert!(
        lamp.read(&format!("/etc/systemd/system/{TASKS}"))
            .contains(&sha("tasks 1"))
    );
    assert_eq!(
        tasks_calls(&lamp),
        [
            "harness fetch".to_owned(),
            "harness link".to_owned(),
            format!("restart {TASKS}"),
            format!("stop {TASKS}"),
            "harness off".to_owned(),
        ]
    );
}

#[test]
fn harness_already_on_the_lamp_is_kept_as_it_is() {
    let mut lamp = tasks_lamp("tasks 1");
    lamp.harness.present = true;
    publish(&mut lamp, true, Some(manifest::HARNESS_UPDATER));
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(lamp.harness.asked.is_empty());
    assert!(tasks_calls(&lamp).is_empty(), "{:?}", lamp.calls());
    assert!(!lamp.exists(DONE));
}

#[test]
fn harness_that_cannot_be_fetched_is_said_and_the_build_switches_without_it() {
    let mut lamp = tasks_lamp("tasks 1");
    lamp.harness.fetch_fails = true;
    publish(&mut lamp, true, Some(manifest::HARNESS_UPDATER));
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert_eq!(lamp.harness.asked, ["fetch"]);
    assert!(
        lamp.said.iter().any(|line| line.starts_with(
            "Harness could not be fetched for this build: curl: (6) Could not resolve host; the build goes on without it"
        )),
        "{:?}",
        lamp.said
    );
    assert!(!lamp.exists(DONE));
    assert!(
        !lamp.calls().contains(&format!("restart {TASKS}")),
        "{:?}",
        lamp.calls()
    );
}

#[test]
fn a_tasks_part_not_well_on_harness_puts_the_build_and_harness_back() {
    let mut lamp = tasks_lamp("tasks 1 crash");
    publish(&mut lamp, true, Some(manifest::HARNESS_UPDATER));
    match lamp.round() {
        Outcome::RolledBack { sequence: 2, .. } => {}
        other => panic!("{other:?}: {:?}", lamp.said),
    }
    assert_eq!(lamp.harness.asked, ["fetch", "link", "off"]);
    let calls = tasks_calls(&lamp);
    let at = |call: &str| calls.iter().position(|made| made == call).unwrap();
    assert!(
        at(&format!("stop {TASKS}")) < at("harness off"),
        "{calls:?}"
    );
    assert!(!lamp.exists(DONE));
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
    // The voice is back on build 1.
    assert!(
        lamp.read("/etc/systemd/system/lamp-voice.service")
            .contains(&sha("voice 1"))
    );
}

#[test]
fn harness_that_cannot_be_linked_puts_the_build_back() {
    let mut lamp = tasks_lamp("tasks 1");
    lamp.harness.link_fails = true;
    publish(&mut lamp, true, Some(manifest::HARNESS_UPDATER));
    match lamp.round() {
        Outcome::RolledBack {
            sequence: 2, why, ..
        } => {
            assert!(why.starts_with("Harness could not be turned on"), "{why}");
        }
        other => panic!("{other:?}: {:?}", lamp.said),
    }
    assert_eq!(lamp.harness.asked, ["fetch", "link", "off"]);
    assert!(!lamp.exists(DONE));
    assert!(
        !lamp.calls().contains(&format!("restart {TASKS}")),
        "{:?}",
        lamp.calls()
    );
}

#[test]
fn a_switch_cut_short_after_harness_was_fetched_turns_it_off_on_recovery() {
    let mut lamp = tasks_lamp("tasks 1");
    publish(&mut lamp, true, Some(manifest::HARNESS_UPDATER));
    lamp.systemd.cut_at_reload = true;
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lamp.round()));
    assert!(cut.is_err());
    let journal: serde_json::Value =
        serde_json::from_str(&lamp.read("/var/lib/lampos-update/journal.json")).unwrap();
    assert_eq!(journal["harness"], true);
    assert_eq!(lamp.harness.asked, ["fetch"]);
    match lamp.round() {
        Outcome::RolledBack { sequence: 2, .. } => {}
        other => panic!("{other:?}: {:?}", lamp.said),
    }
    assert_eq!(lamp.harness.asked, ["fetch", "off"]);
    assert!(!lamp.exists(DONE));
    assert!(!lamp.exists("/var/lib/lampos-update/journal.json"));
}

#[test]
fn a_build_that_turns_harness_on_below_its_updater_level_is_refused_before_anything_is_fetched() {
    let mut lamp = tasks_lamp("tasks 1");
    publish(&mut lamp, true, Some(manifest::HARNESS_UPDATER - 1));
    match lamp.round() {
        Outcome::Refused(why) => assert!(
            why.contains(&format!(
                "a build that turns Harness on needs min_updater {}",
                manifest::HARNESS_UPDATER
            )),
            "{why}"
        ),
        other => panic!("{other:?}: {:?}", lamp.said),
    }
    assert!(lamp.harness.asked.is_empty());
    assert!(
        !lamp.calls().iter().any(|call| call.starts_with("restart")),
        "{:?}",
        lamp.calls()
    );
    assert!(
        lamp.read("/etc/systemd/system/lamp-voice.service")
            .contains(&sha("voice 1"))
    );
}
