//! An updater build put back is tried once more only when a reboot came
//! in its minutes (#2810): its switch began on one boot and
//! lamp-update-back put it back on another.
use super::*;

const BAD: &str = "#!/bin/sh\nexit 1\n";

fn boot(lamp: &Lamp, id: &str) {
    let path = lamp.root.join(own::BOOT_ID.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("{id}\n")).unwrap();
}

/// A switch to the build carrying BAD, begun on boot `on`, then put back
/// by lamp-update-back as not running.
fn switched_and_put_back(lamp: &mut Lamp, on: &str, then: &str) {
    boot(lamp, on);
    assert!(
        matches!(lamp.round(), Outcome::SelfSwitched { .. }),
        "{:?}",
        lamp.said
    );
    boot(lamp, then);
    let back = lamp.back(update_unit("failed", "failed"), records_now());
    assert!(matches!(back, own::Back::PutBack(_)), "{back:?}");
}

#[test]
fn a_build_put_back_on_the_boot_it_began_on_stays_refused() {
    let mut lamp = Lamp::installed_switching_itself();
    let old = lamp.own_build();
    product_build_with_updater(&mut lamp, 2, BAD);
    switched_and_put_back(&mut lamp, "boot-a", "boot-a");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    // An ordinary reboot hours later does not reopen it (Tests' P2 on
    // #4131).
    boot(&lamp, "boot-b");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(lamp.own_build(), old);
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
        lamp.said
            .iter()
            .any(|line| line.contains("tried once more")),
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
    let back = lamp.back(update_unit("failed", "failed"), records_now());
    assert!(matches!(back, own::Back::PutBack(_)), "{back:?}");
    boot(&lamp, "boot-b");
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
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
    assert!(own::refused_for_good(&lamp.records(), &sha(BAD)));
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
    assert!(own::refused_for_good(&lamp.records(), &sha(BAD)));
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
