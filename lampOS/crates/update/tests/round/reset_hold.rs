//! No switch while a reset of the lamp may be under way, or in the first
//! seconds after the boot (#6266, 5d): the round holds (`NotQuiet`), is not
//! refused, and the next round takes the build once the reset is over.
use super::*;

fn publish_voice(lamp: &mut Lamp, sequence: u64) {
    lamp.publish(
        sequence,
        &[
            ("lamp-voice", &format!("voice {sequence}")),
            ("lamp-turn-end", &format!("turn-end {sequence}")),
            ("lamp-body", "body 1"),
            ("lamp-controls", "controls 1"),
        ],
        &[],
    );
}

fn uptime(lamp: &Lamp, text: &str) {
    let path = lamp.root.join("proc/uptime");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn reset_journal(lamp: &Lamp) -> std::path::PathBuf {
    let path = lamp.root.join("var/lib/lampos-setup/reset.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    path
}

/// How many builds are refused: none, and no state kept either, for a round held
/// before it wrote anything.
fn refused(lamp: &Lamp) -> usize {
    if !lamp.exists("/var/lib/lampos-update/state.json") {
        return 0;
    }
    lamp.state()["refused"].as_array().map_or(0, Vec::len)
}

/// A reset's journal is there from its start until its wipe is done: no switch
/// then, no refusal, and the next round after it is gone takes the build.
#[test]
fn a_reset_journal_holds_the_round_and_nothing_is_switched_or_refused() {
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    let journal = reset_journal(&lamp);
    fs::write(&journal, "{}").unwrap();
    let calls = lamp.calls().len();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::NotQuiet(why) if why.contains("reset of the lamp")),
        "{outcome:?}"
    );
    assert_eq!(lamp.calls().len(), calls, "nothing was restarted");
    assert_eq!(refused(&lamp), 0);
    // The wipe is done and the journal gone: the build is taken.
    fs::remove_file(&journal).unwrap();
    assert_eq!(lamp.round(), Outcome::Switched(2));
}

/// The first 180 s after the boot hold the round; later, or with no uptime that
/// reads, it goes on.
#[test]
fn the_first_minutes_after_the_boot_hold_the_round() {
    for (text, held) in [
        ("12.50 30.00\n", true),
        ("179.9 100.0\n", true),
        ("180.0 100.0\n", false),
        ("86400.0 1.0\n", false),
        ("garbage\n", false),
        ("", false),
        ("-3 1\n", false),
    ] {
        let mut lamp = Lamp::new();
        publish_voice(&mut lamp, 2);
        uptime(&lamp, text);
        let outcome = lamp.round();
        if held {
            assert!(
                matches!(&outcome, Outcome::NotQuiet(why) if why.contains("booted")),
                "{text:?}: {outcome:?}"
            );
            assert_eq!(refused(&lamp), 0);
        } else {
            assert_eq!(outcome, Outcome::Switched(2), "{text:?}");
        }
    }
    // No /proc/uptime at all (the harness lamp's default): never held.
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    assert_eq!(lamp.round(), Outcome::Switched(2));
}

/// The first update after setup (#6284) is forced and wanted right after the
/// boot's setup: the boot's grace does not hold it, or it would end `not-quiet`
/// and be lost; a reset's journal still does.
#[test]
fn the_first_update_owed_is_not_held_by_the_boots_grace_but_by_a_reset() {
    let now = lamp_update::records::unix_ms();
    let owed = |lamp: &mut Lamp| {
        publish_voice(lamp, 2);
        lamp.config.first_update = true;
        let path = lamp
            .root
            .join(lamp_msgs::FIRST_UPDATE_FILE.trim_start_matches('/'));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!(r#"{{"at_unix_ms":{now}}}"#)).unwrap();
        let done = lamp.root.join("var/lib/lampos-setup/done");
        fs::create_dir_all(done.parent().unwrap()).unwrap();
        fs::write(done, "").unwrap();
        uptime(lamp, "20.0 40.0\n");
    };
    let mut lamp = Lamp::new();
    owed(&mut lamp);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    let mut lamp = Lamp::new();
    owed(&mut lamp);
    fs::write(reset_journal(&lamp), "{}").unwrap();
    let outcome = lamp.round();
    assert!(
        matches!(&outcome, Outcome::NotQuiet(why) if why.contains("reset of the lamp")),
        "{outcome:?}"
    );
}

fn marker(lamp: &Lamp) {
    let done = lamp.root.join("var/lib/lampos-setup/done");
    fs::create_dir_all(done.parent().unwrap()).unwrap();
    fs::write(done, "").unwrap();
}

fn dated(path: &std::path::Path, ago: Option<std::time::Duration>) {
    let when = match ago {
        Some(ago) => std::time::SystemTime::now() - ago,
        // Dated ahead: a clock that jumped.
        None => std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
    };
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

/// The Reviewer-lead's P2: a journal left beside the setup marker (a failed
/// unlink of the marker, a restore) is not a reset and must not hold every round
/// for good. The real flow has the two together for milliseconds: a journal holds
/// when the marker is missing, or when it is new; otherwise it is said and the
/// round goes on.
#[test]
fn a_stale_journal_beside_the_marker_is_said_and_holds_nothing() {
    let hour = std::time::Duration::from_secs(3600);
    let held =
        |why: &Outcome| matches!(why, Outcome::NotQuiet(why) if why.contains("reset of the lamp"));
    // The marker there, the journal an hour old: no reset. Said, and taken.
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    marker(&lamp);
    let journal = reset_journal(&lamp);
    fs::write(&journal, "{}").unwrap();
    dated(&journal, Some(hour));
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("is beside the setup marker")),
        "{:?}",
        lamp.said
    );
    // The marker there, the journal new (a reset begun now): held.
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    marker(&lamp);
    let journal = reset_journal(&lamp);
    fs::write(&journal, "{}").unwrap();
    let outcome = lamp.round();
    assert!(held(&outcome), "{outcome:?}");
    assert!(
        !lamp
            .said
            .iter()
            .any(|line| line.contains("beside the setup marker"))
    );
    // The marker gone, the journal an hour old: held (the lamp is being emptied,
    // or setup finishes the wipe at its next start).
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    let journal = reset_journal(&lamp);
    fs::write(&journal, "{}").unwrap();
    dated(&journal, Some(hour));
    let outcome = lamp.round();
    assert!(held(&outcome), "{outcome:?}");
    // The marker there, the journal dated ahead: no age, so not new, so not held.
    let mut lamp = Lamp::new();
    publish_voice(&mut lamp, 2);
    marker(&lamp);
    let journal = reset_journal(&lamp);
    fs::write(&journal, "{}").unwrap();
    dated(&journal, None);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
}
