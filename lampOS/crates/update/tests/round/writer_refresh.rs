//! The settings writer refreshed from the running lamp-update before a
//! round looks at a build (#3689 item 1): asked under the round's lock, and
//! done by the writer's one-shot unit.
use super::*;
use lamp_update::{fixed_copy, writer_swap, writer_unit};

const BACK: &str = "/usr/local/lib/lampos/lamp-update-back";
const PREV: &str = "/usr/local/lib/lampos/lamp-update-back.prev";

/// An installed lamp whose writer is an older copy than the lamp-update
/// that runs, a round of which refreshes it; its systemd runs the writer's
/// unit as the unit would, in this process.
fn lamp() -> Lamp {
    let mut lamp = Lamp::installed_switching_itself();
    fs::write(lamp.root.join(BACK.trim_start_matches('/')), "old writer").unwrap();
    lamp.config.writer_from = Some(writer_swap::Running::new(b"lamp-update 1".to_vec()));
    let (root, records) = (lamp.root.clone(), lamp.records());
    lamp.systemd.writer = Some(Box::new(move || {
        writer_swap::refresh(
            &root,
            &records,
            b"lamp-update 1",
            lamp_update::records::unix_ms(),
            &mut |path: &Path| Ok(fixed_copy::answer(&sha256_hex(&fs::read(path).unwrap()))),
        )
        .map(|_| ())
    }));
    lamp
}

fn started_writer(lamp: &Lamp) -> bool {
    lamp.calls()
        .contains(&format!("start {}", writer_unit::UNIT))
}

#[test]
fn a_round_refreshes_an_older_writer_from_the_lamp_update_that_runs_first() {
    let mut lamp = lamp();
    lamp.round();
    assert!(started_writer(&lamp), "{:?}", lamp.calls());
    assert_eq!(lamp.read(BACK), "lamp-update 1");
    assert_eq!(lamp.read(PREV), "old writer");
    assert_eq!(
        lamp.read(&format!("/etc/systemd/system/{}", writer_unit::UNIT)),
        writer_unit::TEXT
    );
    assert!(!lamp.exists("/var/lib/lampos-update/writer-refresh.request"));
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with("the settings writer is now build")),
        "{:?}",
        lamp.said
    );
    // Once it is this build, the next round asks nothing.
    lamp.systemd.calls.borrow_mut().clear();
    lamp.round();
    assert!(!started_writer(&lamp), "{:?}", lamp.calls());
}

#[test]
fn a_writer_unit_that_cannot_run_leaves_the_writer_and_takes_its_request_back() {
    let mut lamp = lamp();
    lamp.systemd.writer = None;
    lamp.round();
    assert_eq!(lamp.read(BACK), "old writer");
    assert!(!lamp.exists(PREV));
    assert!(!lamp.exists("/var/lib/lampos-update/writer-refresh.request"));
    assert!(
        lamp.said
            .iter()
            .any(|line| line.starts_with("the settings writer is not refreshed")),
        "{:?}",
        lamp.said
    );
}

#[test]
fn a_dry_round_or_a_lamp_with_no_writer_or_no_bytes_given_refreshes_nothing() {
    // A dry round (`check`).
    let mut lamp = lamp();
    lamp.config.apply = false;
    lamp.round();
    assert!(!started_writer(&lamp));
    assert_eq!(lamp.read(BACK), "old writer");
    // A lamp installed before the writer: it is install's to put there.
    let mut lamp = super::writer_refresh::lamp();
    fs::remove_file(lamp.root.join(BACK.trim_start_matches('/'))).unwrap();
    lamp.round();
    assert!(!started_writer(&lamp));
    assert!(!lamp.exists(BACK));
    // A round given no bytes of its own (every other test's).
    let mut lamp = super::writer_refresh::lamp();
    lamp.config.writer_from = None;
    lamp.round();
    assert!(!started_writer(&lamp));
}

/// The lamp after a round that refreshed its writer.
fn refreshed() -> Lamp {
    let mut lamp = lamp();
    lamp.round();
    assert_eq!(lamp.read(BACK), "lamp-update 1");
    assert!(writer_swap::unproven(&lamp.root, &lamp.records()));
    lamp.systemd.calls.borrow_mut().clear();
    lamp
}

#[test]
fn a_refreshed_writer_whose_back_failed_is_put_back_by_the_next_round() {
    let mut lamp = refreshed();
    lamp.systemd.units.insert(
        writer_swap::BACK_UNIT.into(),
        Running {
            active: "failed".into(),
            sub: "failed".into(),
            ..Running::default()
        },
    );
    lamp.round();
    assert_eq!(lamp.read(BACK), "old writer");
    assert!(!lamp.exists(PREV));
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("is put back (its back failed)")),
        "{:?}",
        lamp.said
    );
    // This build is never asked to be the writer again.
    lamp.systemd.calls.borrow_mut().clear();
    lamp.round();
    assert!(!started_writer(&lamp), "{:?}", lamp.calls());
}

#[test]
fn a_refreshed_writer_whose_back_ended_well_since_is_proved_and_its_prev_removed() {
    let mut lamp = refreshed();
    let done = writer_swap::done(&lamp.records()).unwrap();
    writer_swap::note_back(&lamp.records(), b"lamp-update 1", done.at_unix_ms + 1).unwrap();
    lamp.round();
    assert_eq!(lamp.read(BACK), "lamp-update 1");
    assert!(!lamp.exists(PREV));
    assert!(!writer_swap::unproven(&lamp.root, &lamp.records()));
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("has proved itself")),
        "{:?}",
        lamp.said
    );
}

#[test]
fn the_writers_unit_is_never_taken_for_a_part_so_update_current_moves_only_by_a_self_switch() {
    let mut lamp = refreshed();
    let before = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    lamp.round();
    // The parts switched; update/current did not move as a part's link.
    assert_eq!(lamp.own_build(), before);
    assert!(
        !lamp
            .calls()
            .contains(&format!("restart {}", writer_unit::UNIT))
    );
}

#[test]
fn no_switch_of_lamp_updates_own_leans_on_a_writer_not_proved_yet() {
    let mut lamp = refreshed();
    product_build_with_updater(&mut lamp, 2, "lamp-update 2");
    let outcome = lamp.round();
    assert!(
        !matches!(outcome, Outcome::SelfSwitched { .. }),
        "{outcome:?}"
    );
    assert!(lamp.pending().is_none());
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("own switch waits: the settings writer")),
        "{outcome:?} {:?}",
        lamp.said.iter().rev().take(3).collect::<Vec<_>>()
    );
}

#[test]
fn a_new_writer_whose_first_apply_fails_is_put_back_after_that_apply() {
    let (mut lamp, _) = lamp_with_settings();
    // update/current names the lamp-update that runs; an older writer is at
    // the fixed path.
    let hash = sha("lamp-update 1");
    let folder = lamp.root.join(format!("opt/lampOS/update/{hash}"));
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("lamp-update"), "lamp-update 1").unwrap();
    std::os::unix::fs::symlink(&hash, lamp.root.join("opt/lampOS/update/current")).unwrap();
    let back = lamp.root.join(BACK.trim_start_matches('/'));
    fs::create_dir_all(back.parent().unwrap()).unwrap();
    fs::write(&back, "old writer").unwrap();
    lamp.config.writer_from = Some(writer_swap::Running::new(b"lamp-update 1".to_vec()));
    let (root, records) = (lamp.root.clone(), lamp.records());
    lamp.systemd.writer = Some(Box::new(move || {
        writer_swap::refresh(
            &root,
            &records,
            b"lamp-update 1",
            lamp_update::records::unix_ms(),
            &mut |path: &Path| Ok(fixed_copy::answer(&sha256_hex(&fs::read(path).unwrap()))),
        )
        .map(|_| ())
    }));
    // The new writer's first apply fails before it writes anything.
    lamp.systemd.applier = Some(Box::new(|| {
        Err("lamp-update-apply.service: exit status: 1".into())
    }));
    publish_settings(
        &mut lamp,
        2,
        vec![Setting::new(CONTROLS_JSON, "controls 2")],
    );
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::RolledBack { sequence: 2, .. }),
        "{outcome:?}"
    );
    assert_eq!(lamp.read(BACK), "old writer");
    assert!(!lamp.exists(PREV));
    assert_eq!(lamp.read(CONTROLS_JSON), "controls 1");
    let said = |text: &str| lamp.said.iter().position(|line| line.contains(text));
    let (refreshed, put_back) = (
        said("the settings writer is now build"),
        said("is put back (its first apply failed)"),
    );
    assert!(refreshed.is_some() && put_back.is_some(), "{:?}", lamp.said);
    assert!(refreshed < put_back);
}
