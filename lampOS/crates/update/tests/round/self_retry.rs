//! An updater build put back is tried once more, once (#2810, #6901): when
//! a reboot came in its minutes (its switch began on one boot and
//! lamp-update-back put it back on another), when the lamp is on a later
//! boot than the one it was put back on, or 24 hours after the put-back.
//! The owner's retry from the app lets every build put back be tried again.
use super::*;
use std::time::SystemTime;

const BAD: &str = "#!/bin/sh\nexit 1\n";
/// How long a build put back stays refused on the boot of its put-back.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn boot(lamp: &Lamp, id: &str) {
    let path = lamp.root.join(own::BOOT_ID.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("{id}\n")).unwrap();
}

/// lamp-update-back, started for a lamp-update that is not running.
fn put_back(lamp: &mut Lamp) {
    let back = lamp.back(update_unit("failed", "failed"), records_now());
    assert!(matches!(back, own::Back::PutBack(_)), "{back:?}");
}

/// A switch to the build carrying BAD, begun on boot `on`, then put back
/// by lamp-update-back as not running.
fn switched_and_put_back(lamp: &mut Lamp, on: &str, then: &str) {
    boot(lamp, on);
    let outcome = lamp.round();
    assert!(
        matches!(outcome, Outcome::SelfSwitched { .. }),
        "{outcome:?}: {:?}",
        lamp.said
    );
    boot(lamp, then);
    put_back(lamp);
}

/// `self-put-back` dated `at`: the file's own time is when the build was
/// put back (#6901).
fn put_back_dated(lamp: &Lamp, at: SystemTime) {
    fs::File::options()
        .write(true)
        .open(lamp.records().join("self-put-back"))
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// Whether a round said it tries BAD's build once more, `why` being the
/// words after "was put back".
fn said_once_more(lamp: &Lamp, why: &str) -> bool {
    let line = format!(
        "lamp-update's build {} was put back {why}: it is tried once more, and a second put-back is final",
        &sha(BAD)[..12]
    );
    lamp.said.contains(&line)
}

/// The owner's retry of build `sequence` from the app, as `serve` takes it
/// before its next round: what it said.
fn owner_retries(lamp: &Lamp, sequence: u64) -> Vec<String> {
    let _no_fork = no_fork();
    let records = lamp_update::records::Records::new(lamp.records());
    let mut told = Vec::new();
    lamp_update::app_check::drop_refusals(&records, &[sequence], &mut |line| {
        told.push(line.to_owned())
    });
    told
}

#[test]
fn a_build_put_back_on_the_boot_it_began_on_stays_refused_on_that_boot_for_24_hours() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, BAD);
    switched_and_put_back(&mut lamp, "boot-a", "boot-a");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    // An hour short of 24 hours later, on that boot still.
    put_back_dated(&lamp, SystemTime::now() - (DAY - Duration::from_secs(3600)));
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
    assert_eq!(own::refused(&lamp.records()), vec![sha(BAD)]);
    assert!(own::retried(&lamp.records()).is_empty());
}

/// #6901: a Wi-Fi drop or a refused download in a switch's ten minutes
/// reads to lamp-update-back as a build that cannot run, and the lamp
/// stayed on it for good. Now the lamp's next start tries it once more.
#[test]
fn a_build_put_back_on_the_boot_it_began_on_is_tried_once_more_on_a_later_boot() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    let new = sha(BAD);
    product_build_with_updater(&mut lamp, 2, BAD);
    switched_and_put_back(&mut lamp, "boot-a", "boot-a");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    // The lamp starts again: tried once more, and said with why.
    switched_and_put_back(&mut lamp, "boot-b", "boot-b");
    assert!(
        said_once_more(&lamp, "and the lamp is on a later boot now"),
        "{:?}",
        lamp.said
    );
    assert_eq!(own::retried(&lamp.records()), vec![new.clone()]);
    assert_eq!(own::refused(&lamp.records()), vec![new]);
    // Put back a second time: never again by itself, on a later boot and
    // days later too.
    boot(&lamp, "boot-c");
    put_back_dated(&lamp, SystemTime::now() - 3 * DAY);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
}

/// #6901: a lamp that is never restarted tries it once more a day later,
/// by the time `self-put-back` was written.
#[test]
fn a_build_put_back_on_the_boot_it_began_on_is_tried_once_more_after_24_hours() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    let new = sha(BAD);
    product_build_with_updater(&mut lamp, 2, BAD);
    switched_and_put_back(&mut lamp, "boot-a", "boot-a");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    // 24 hours after the put-back, on the same boot.
    put_back_dated(&lamp, SystemTime::now() - DAY);
    switched_and_put_back(&mut lamp, "boot-a", "boot-a");
    assert!(
        said_once_more(&lamp, "24 hours ago or more"),
        "{:?}",
        lamp.said
    );
    assert_eq!(own::retried(&lamp.records()), vec![new.clone()]);
    assert_eq!(own::refused(&lamp.records()), vec![new]);
    // Put back a second time: final, however long ago.
    put_back_dated(&lamp, SystemTime::now() - 3 * DAY);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
}

/// A lamp whose clock went back since the put-back (no clock of its own,
/// and no network yet) reads the put-back as after now: that is not 24
/// hours passed.
#[test]
fn a_put_back_dated_after_now_is_not_24_hours_ago() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, BAD);
    switched_and_put_back(&mut lamp, "boot-a", "boot-a");
    put_back_dated(&lamp, SystemTime::now() + 3 * DAY);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
    assert!(own::retried(&lamp.records()).is_empty());
}

#[test]
fn a_build_put_back_after_a_reboot_is_tried_once_more_and_a_second_put_back_is_final() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    let new = sha(BAD);
    product_build_with_updater(&mut lamp, 2, BAD);
    // A power cut in its minutes: put back on the next boot.
    switched_and_put_back(&mut lamp, "boot-a", "boot-b");
    assert_eq!(own::refused(&lamp.records()), vec![new.clone()]);
    // Tried once more, and said.
    switched_and_put_back(&mut lamp, "boot-b", "boot-c");
    assert!(
        said_once_more(&lamp, "after a reboot came in its minutes"),
        "{:?}",
        lamp.said
    );
    assert_eq!(own::retried(&lamp.records()), vec![new.clone()]);
    // Put back a second time: never again, whatever the boot.
    boot(&lamp, "boot-d");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
}

#[test]
fn with_no_boot_known_a_build_put_back_stays_refused() {
    let mut lamp = Lamp::installed_switching_itself();
    product_build_with_updater(&mut lamp, 2, BAD);
    assert!(matches!(lamp.round(), Outcome::SelfSwitched { .. }));
    put_back(&mut lamp);
    assert!(!lamp.records().join("self-put-back").exists());
    boot(&lamp, "boot-b");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    // A put-back noted for another build, days ago and on another boot,
    // says nothing of this one.
    fs::write(
        lamp.records().join("self-put-back"),
        format!("{} boot-a\n", sha("another lamp-update")),
    )
    .unwrap();
    put_back_dated(&lamp, SystemTime::now() - 3 * DAY);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert!(own::retried(&lamp.records()).is_empty());
}

/// #6901: the owner's retry from the app lets every lamp-update build put
/// back be tried again, one refused for good too; put back after it, that
/// one is refused for good again.
#[test]
fn the_owners_retry_lets_a_build_refused_for_good_be_tried_again() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    let new = sha(BAD);
    product_build_with_updater(&mut lamp, 2, BAD);
    // Put back, tried once more, put back again: final.
    switched_and_put_back(&mut lamp, "boot-a", "boot-b");
    switched_and_put_back(&mut lamp, "boot-b", "boot-c");
    boot(&lamp, "boot-d");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);

    let told = owner_retries(&lamp, 2);
    assert_eq!(
        told,
        [
            "the owner asked to retry build 2: it is not refused, so a plain check",
            "the owner asked to retry: 1 of lamp-update's own builds put back before may be tried again",
        ]
    );
    assert!(own::refused(&lamp.records()).is_empty());
    assert_eq!(own::retried(&lamp.records()), vec![new.clone()]);

    // The round the retry starts takes it again. Put back after it, it is
    // refused for good again: on a later boot and days later too.
    switched_and_put_back(&mut lamp, "boot-d", "boot-d");
    assert_eq!(own::refused(&lamp.records()), vec![new.clone()]);
    assert_eq!(own::retried(&lamp.records()), vec![new]);
    boot(&lamp, "boot-e");
    put_back_dated(&lamp, SystemTime::now() - 3 * DAY);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);

    // Asked again, it is let go again; with none put back, nothing is said
    // of lamp-update's own builds.
    assert_eq!(owner_retries(&lamp, 2).len(), 2);
    assert_eq!(
        owner_retries(&lamp, 2),
        ["the owner asked to retry build 2: it is not refused, so a plain check"]
    );
}

/// #6901's lamp: its updater is too old for the build, climbs to the one
/// the build carries, and that one is put back on the boot of its switch.
/// Every round then said "waiting for a newer updater", for good. Now it
/// climbs once more on a later boot, and again at the owner's retry.
#[test]
fn a_climb_put_back_on_its_boot_climbs_once_more_on_a_later_boot_and_at_the_owners_retry() {
    let mut lamp = Lamp::installed_switching_itself();
    lamp.config.updater = 1;
    let old = lamp.own_build();
    let build = product_build_with_updater(&mut lamp, 2, BAD);
    needing(&mut lamp, build, 2);
    let climbs = |lamp: &mut Lamp| {
        let outcome = lamp.round();
        assert!(
            matches!(outcome, Outcome::Climbs { .. }),
            "{outcome:?}: {:?}",
            lamp.said
        );
    };
    let waits = |lamp: &mut Lamp| {
        let outcome = lamp.round();
        assert!(
            matches!(&outcome, Outcome::Refused(why) if why.ends_with("waiting for a newer updater")),
            "{outcome:?}: {:?}",
            lamp.said
        );
        assert_eq!(lamp.own_build(), old);
    };
    boot(&lamp, "boot-a");
    climbs(&mut lamp);
    put_back(&mut lamp);
    waits(&mut lamp);

    // The lamp starts again: it climbs once more, and says why.
    boot(&lamp, "boot-b");
    climbs(&mut lamp);
    assert!(
        said_once_more(&lamp, "and the lamp is on a later boot now"),
        "{:?}",
        lamp.said
    );
    put_back(&mut lamp);
    // Put back a second time: final, on a later boot and days later too.
    boot(&lamp, "boot-c");
    put_back_dated(&lamp, SystemTime::now() - 3 * DAY);
    waits(&mut lamp);

    // The owner's retry: it climbs again.
    let told = owner_retries(&lamp, 2);
    assert!(
        told.iter().any(|line| line
            == "the owner asked to retry: 1 of lamp-update's own builds put back before may be tried again"),
        "{told:?}"
    );
    climbs(&mut lamp);
    put_back(&mut lamp);
    waits(&mut lamp);
}

/// A lamp whose updater build was put back for good still takes a newer,
/// different lamp-update when the channel carries one: by its own switch,
/// and by a climb when the build needs a newer updater.
#[test]
fn an_updater_refused_for_good_does_not_block_a_newer_updater() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, BAD);
    switched_and_put_back(&mut lamp, "boot-a", "boot-a");
    assert!(own::refused_for_good(
        &lamp.records(),
        &lamp.root,
        &sha(BAD)
    ));
    product_build_with_updater(&mut lamp, 3, "lamp-update 3");
    assert_eq!(
        lamp.round(),
        Outcome::SelfSwitched {
            sequence: 3,
            from: old,
            to: sha("lamp-update 3")
        },
        "{:?}",
        lamp.said
    );

    // Too old for the build: the climb to BAD is put back for good, and a
    // later build carrying a fixed lamp-update is climbed to.
    let mut lamp = Lamp::installed_switching_itself();
    lamp.config.updater = 1;
    let old = lamp.own_build();
    let build = product_build_with_updater(&mut lamp, 2, BAD);
    needing(&mut lamp, build, 2);
    boot(&lamp, "boot-a");
    assert!(
        matches!(lamp.round(), Outcome::Climbs { .. }),
        "{:?}",
        lamp.said
    );
    let back = lamp.back(update_unit("failed", "failed"), records_now());
    assert!(matches!(back, own::Back::PutBack(_)), "{back:?}");
    assert!(own::refused_for_good(
        &lamp.records(),
        &lamp.root,
        &sha(BAD)
    ));
    assert!(
        matches!(lamp.round(), Outcome::Refused(_)),
        "{:?}",
        lamp.said
    );
    let build = product_build_with_updater(&mut lamp, 3, "lamp-update 3");
    needing(&mut lamp, build, 2);
    assert_eq!(
        lamp.round(),
        Outcome::Climbs {
            sequence: 3,
            needed: 2,
            from: old,
            to: sha("lamp-update 3")
        },
        "{:?}",
        lamp.said
    );
}
