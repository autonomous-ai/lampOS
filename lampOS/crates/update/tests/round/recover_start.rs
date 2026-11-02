//! Recovery (`lamp-setup-recover.service`) is enabled by a round that adds
//! it and never started inside one: started and watched, a unit that ends
//! at once with its switch off would read as stopped and put the build
//! back. With its switch on through its drop-ins, a round whose watch passed
//! starts it once, after the watch, so a lamp needn't reboot first (#4863).
use super::*;

const RECOVER: &str = "lamp-setup-recover.service";

const RECOVER_UNIT: &str = "[Service]\nExecStart=/opt/lampOS/setup/current/lamp-setup recover\n\n[Install]\nWantedBy=multi-user.target\n";

/// Recovery's unit on the lamp, enabled or not, and its switch's drop-in
/// saying `switch` (none: no drop-in).
fn with_recover(lamp: &Lamp, enabled: bool, switch: Option<&str>) {
    let units = lamp.root.join("etc/systemd/system");
    fs::write(units.join(RECOVER), RECOVER_UNIT).unwrap();
    if enabled {
        let wants = units.join("multi-user.target.wants");
        fs::create_dir_all(&wants).unwrap();
        std::os::unix::fs::symlink(units.join(RECOVER), wants.join(RECOVER)).unwrap();
    }
    if let Some(switch) = switch {
        let folder = units.join(format!("{RECOVER}.d"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("on.conf"),
            format!("[Service]\nEnvironment=LAMP_WIFI_RECOVERY={switch}\n"),
        )
        .unwrap();
    }
}

fn starts(lamp: &Lamp) -> usize {
    lamp.calls()
        .iter()
        .filter(|call| *call == &format!("start {RECOVER}"))
        .count()
}

#[test]
fn a_round_that_passes_its_watch_starts_recover_once_after_it_when_its_switch_is_on() {
    let mut lamp = Lamp::new();
    with_recover(&lamp, true, Some("on"));
    build_two(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(
        lamp.calls(),
        [
            "reload",
            "restart lamp-voice.service",
            &format!("start {RECOVER}")
        ]
    );
    assert!(
        lamp.said.iter().any(|line| line.contains(RECOVER)),
        "{:?}",
        lamp.said
    );
    // A round with nothing to switch starts nothing.
    assert_eq!(lamp.round(), Outcome::UpToDate(2));
    assert_eq!(starts(&lamp), 1);
}

#[test]
fn recover_off_or_not_enabled_is_not_started() {
    for (enabled, switch) in [(true, None), (true, Some("off")), (false, Some("on"))] {
        let mut lamp = Lamp::new();
        with_recover(&lamp, enabled, switch);
        build_two(&mut lamp);
        assert_eq!(lamp.round(), Outcome::Switched(2));
        assert_eq!(starts(&lamp), 0, "{enabled} {switch:?}");
    }
}

#[test]
fn a_recover_that_ends_at_once_is_never_watched_and_puts_nothing_back() {
    let mut lamp = Lamp::new();
    with_recover(&lamp, true, Some("on"));
    lamp.systemd.crashing.push(RECOVER.into());
    build_two(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    // Started only once the watch had passed and the build was kept.
    assert_eq!(lamp.calls().last(), Some(&format!("start {RECOVER}")));
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
    assert!(lamp.state()["refused"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn a_start_that_fails_is_said_and_the_round_stays_switched() {
    let mut lamp = Lamp::new();
    with_recover(&lamp, true, Some("on"));
    lamp.systemd.start_refused = Some("Job for lamp-setup-recover.service failed".into());
    build_two(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2));
    assert_eq!(starts(&lamp), 1);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("could not be started") && line.contains("failed")),
        "{:?}",
        lamp.said
    );
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
}
