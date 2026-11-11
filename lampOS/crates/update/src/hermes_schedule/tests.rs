use super::*;
use lamp_msgs::Voice;

const MIN: u64 = 60_000;
const S: u64 = 1000;

/// The lead's rule (V2, 15:44 on #5978), as V1 did: the first round
/// reached installs, with no quiet stretch, no night and no wait for the
/// walk to end; whatever the voice is doing, speech only pauses it.
#[test]
fn the_first_reached_round_installs_with_no_quiet() {
    assert_eq!(
        window(&Moment {
            walk_under_way: false
        }),
        Ok(Window::Reached)
    );
}

/// The one exception: the setup walk itself.
#[test]
fn the_setup_walk_blocks_it() {
    assert_eq!(
        window(&Moment {
            walk_under_way: true
        })
        .unwrap_err(),
        crate::walk::UNDER_WAY
    );
}

fn read(boot: &str) -> Lease {
    Lease::Fresh {
        boot: boot.to_owned(),
        physical: Some(false),
        no_slide: false,
    }
}

const NOT_QUIET: fn() -> Result<(), String> = || Err("the voice is hearing someone".to_owned());

/// Someone heard freezes the install at once; it stays frozen while the
/// voice is not quiet and thaws when it has rested again; each freeze is
/// one act, never repeated.
#[test]
fn someone_heard_freezes_it_and_rest_thaws_it() {
    let mut watch = Watch::default();
    assert_eq!(watch.look(0, Ok(()), &read("a")), Act::Nothing);
    assert_eq!(
        watch.look(100, NOT_QUIET(), &read("a")),
        Act::Freeze("the voice is hearing someone".into())
    );
    assert!(watch.frozen());
    assert_eq!(watch.look(200, NOT_QUIET(), &read("a")), Act::Nothing);
    assert_eq!(watch.look(MIN, NOT_QUIET(), &read("a")), Act::Nothing);
    assert_eq!(watch.look(MIN + 100, Ok(()), &read("a")), Act::Thaw);
    assert!(!watch.frozen());
    assert_eq!(watch.look(MIN + 200, Ok(()), &read("a")), Act::Nothing);
}

/// The rest that thaws it is a few seconds of the voice resting after the
/// last word, not a 10-minute stretch (the lead, V2 15:44 on #5978): the
/// voice seen resting and nobody heard for [`RESTED`] (5 s). A voice that
/// says nothing is still not rested.
#[test]
fn a_few_seconds_of_rest_after_speech_thaw_it() {
    assert_eq!(RESTED, Duration::from_secs(5));
    let mut quiet = Quiet::new(0);
    assert!(rested(&quiet, MIN).is_err(), "nothing said");
    for at in (0..=10 * S).step_by(1000) {
        quiet.hear(Heard::State(Voice::Resting), at);
    }
    assert_eq!(rested(&quiet, 10 * S), Ok(()));
    quiet.hear(Heard::Speech, 10 * S);
    assert!(rested(&quiet, 10 * S).is_err(), "speech freezes it");
    for at in (11 * S..=15 * S).step_by(1000) {
        quiet.hear(Heard::State(Voice::Resting), at);
    }
    assert!(rested(&quiet, 15 * S - 1).is_err());
    assert_eq!(rested(&quiet, 15 * S), Ok(()));
}

/// A busy office still finishes (the lead, V2 15:44 on #5978): talk
/// 45 s of every minute for 50 min, rest 15 s, and the install, running
/// 12.5 min in all between turns, is never stopped for its freezes.
#[test]
fn a_long_busy_stretch_still_finishes() {
    let mut watch = Watch::default();
    for minute in 0..50 {
        let at = minute * MIN;
        assert!(
            matches!(watch.look(at, NOT_QUIET(), &read("a")), Act::Freeze(_)),
            "minute {minute}"
        );
        assert_eq!(watch.look(at + 45 * S, Ok(()), &read("a")), Act::Thaw);
    }
    assert!(!watch.frozen());
}

/// No lease read with the slide known in the install's first second is
/// said once, so an install with no privacy watch is not silent about it;
/// a lease read in time says nothing.
#[test]
fn an_install_with_no_privacy_lease_says_so_once() {
    let mut unseen = Watch::default();
    assert_eq!(unseen.look(5_000, Ok(()), &Lease::Gone), Act::Nothing);
    assert_eq!(unseen.no_lease_line(5_000), None);
    assert_eq!(unseen.no_lease_line(5_999), None);
    let line = unseen.no_lease_line(6_000).expect("said after 1 s");
    assert!(line.contains("no privacy watch"), "{line}");
    assert_eq!(unseen.no_lease_line(9_000), None, "said once");
    let mut seen = Watch::default();
    assert_eq!(seen.no_lease_line(0), None);
    assert_eq!(seen.look(100, Ok(()), &read("a")), Act::Nothing);
    assert_eq!(seen.no_lease_line(2_000), None);
}

/// Frozen [`MOST_FROZEN`] (40 min) in all, over one freeze or several,
/// stops it as `interrupted`, tried again in the next round: never frozen
/// for good, and inside the one-shot's hour.
#[test]
fn frozen_too_long_in_all_stops_it() {
    const MOST: u64 = 40 * MIN;
    assert_eq!(MOST_FROZEN.as_millis() as u64, MOST);
    let mut watch = Watch::default();
    assert!(matches!(
        watch.look(0, NOT_QUIET(), &read("a")),
        Act::Freeze(_)
    ));
    assert_eq!(watch.look(MOST - 1, NOT_QUIET(), &read("a")), Act::Nothing);
    match watch.look(MOST, NOT_QUIET(), &read("a")) {
        Act::Stop { error, why } => {
            assert_eq!(error, "interrupted");
            assert!(why.contains("frozen 40 min in all"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!watch.frozen());
    // Two freezes: 30 min, then 10 more.
    let mut watch = Watch::default();
    assert!(matches!(
        watch.look(0, NOT_QUIET(), &read("a")),
        Act::Freeze(_)
    ));
    assert_eq!(watch.look(30 * MIN, Ok(()), &read("a")), Act::Thaw);
    assert!(matches!(
        watch.look(35 * MIN, NOT_QUIET(), &read("a")),
        Act::Freeze(_)
    ));
    assert_eq!(
        watch.look(45 * MIN - 1, NOT_QUIET(), &read("a")),
        Act::Nothing
    );
    assert!(matches!(
        watch.look(45 * MIN, NOT_QUIET(), &read("a")),
        Act::Stop {
            error: "interrupted",
            ..
        }
    ));
}

/// The privacy reading lost stops it at once, frozen or not: the lease
/// gone (older than 250 ms) or no longer reading the slide, or from
/// another of the voice's boots, once one with the slide read was seen. A
/// closed slide is a reading. A lamp whose voice never said a reading is
/// not judged by it, and a lamp with no slide reads by its own lease.
#[test]
fn the_privacy_reading_lost_stops_it_at_once() {
    let unknown = Lease::Fresh {
        boot: "a".into(),
        physical: None,
        no_slide: false,
    };
    let closed = Lease::Fresh {
        boot: "a".into(),
        physical: Some(true),
        no_slide: false,
    };
    for (lost, words) in [
        (Lease::Gone, "gone or older than 250 ms"),
        (unknown.clone(), "no longer reads the slide"),
        (read("b"), "another of its boots"),
    ] {
        for frozen in [false, true] {
            let mut watch = Watch::default();
            assert_eq!(watch.look(0, Ok(()), &read("a")), Act::Nothing);
            assert_eq!(watch.look(50, Ok(()), &closed), Act::Nothing);
            if frozen {
                assert!(matches!(
                    watch.look(100, NOT_QUIET(), &read("a")),
                    Act::Freeze(_)
                ));
            }
            match watch.look(200, Ok(()), &lost) {
                Act::Stop { error, why } => {
                    assert_eq!(error, "privacy_lost");
                    assert!(why.contains(words), "{why}");
                }
                other => panic!("{other:?}"),
            }
            assert!(!watch.frozen());
        }
    }
    // Never read: not judged.
    let mut watch = Watch::default();
    for lease in [Lease::Gone, unknown.clone(), Lease::Gone] {
        assert_eq!(watch.look(0, Ok(()), &lease), Act::Nothing);
    }
    assert_eq!(watch.look(10, Ok(()), &read("a")), Act::Nothing);
    assert!(matches!(
        watch.look(20, Ok(()), &Lease::Gone),
        Act::Stop {
            error: "privacy_lost",
            ..
        }
    ));
    // No slide: its lease is a reading.
    let no_slide = Lease::Fresh {
        boot: "a".into(),
        physical: None,
        no_slide: true,
    };
    let mut watch = Watch::default();
    assert_eq!(watch.look(0, Ok(()), &no_slide), Act::Nothing);
    assert!(matches!(
        watch.look(10, Ok(()), &Lease::Gone),
        Act::Stop {
            error: "privacy_lost",
            ..
        }
    ));
}

/// The bus takes a subscriber named `[a-z][a-z0-9_]{0,31}` only: c0a4 refused
/// `update-hermes`, and the quiet stretch then never heard the voice.
#[test]
fn the_listener_has_a_name_the_bus_takes() {
    let name = super::LISTENER;
    assert!((1..=32).contains(&name.len()), "{name}");
    assert!(name.as_bytes()[0].is_ascii_lowercase(), "{name}");
    assert!(
        name.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "{name}"
    );
}

/// c0a4, 11 Nov: the products stage, frozen 21 min while someone spoke, was
/// taken for timed out the moment it thawed, as a stage's clock goes on
/// while frozen. A Hermes stage gets the installer's own bound and the most
/// it may be frozen; Harness's installer and other steps keep theirs; and
/// the one-shot's hour still holds a stage with every freeze.
#[test]
fn a_hermes_stage_has_room_for_the_most_it_may_be_frozen() {
    use crate::{
        install::{HERMES_STAGE_WITHIN, INSTALLER_WITHIN, bound_of},
        world::RESTART_WITHIN,
    };
    let argv = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let stage = argv(&[
        "runuser",
        "-u",
        "lampos",
        "--",
        "bash",
        &format!("/var/tmp/x/{}", crate::hermes_install::INSTALLER_FILE),
        "--stage",
        "products",
    ]);
    assert_eq!(bound_of(&stage), HERMES_STAGE_WITHIN);
    assert_eq!(HERMES_STAGE_WITHIN, INSTALLER_WITHIN + MOST_FROZEN);
    assert!(HERMES_STAGE_WITHIN < Duration::from_secs(3600));
    assert_eq!(
        bound_of(&argv(&[
            "sh",
            &format!("/x/{}", crate::install::PINNED_INSTALLER)
        ])),
        INSTALLER_WITHIN
    );
    assert_eq!(
        bound_of(&argv(&["systemctl", "restart", "x"])),
        RESTART_WITHIN
    );
}
