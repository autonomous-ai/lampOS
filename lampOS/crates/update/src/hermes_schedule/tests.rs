use super::*;
use lamp_msgs::Voice;

const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
/// 10 Nov 2026, 12:00 UTC: a set clock.
const NOON: u64 = 1_794_312_000_000;

/// A moment that opens the night window: the third round, quiet, no walk,
/// tried once long ago (a lamp that never tried opens by day too), 02:00
/// local.
fn night() -> Moment {
    Moment {
        round: 3,
        quiet: Ok(()),
        walk_under_way: false,
        walk_ended_unix_ms: None,
        tried_unix_ms: Some(NOON - 30 * HOUR),
        now_unix_ms: NOON,
        local_minute: Some(2 * 60),
        asked: false,
    }
}

/// The quiet hour at night opens it: 01:00 up to 05:00 local, not 05:00,
/// not by day, and not without a local time or a set clock.
#[test]
fn the_quiet_hour_at_night_opens_the_window() {
    for minute in [60, 2 * 60, 5 * 60 - 1] {
        let moment = Moment {
            local_minute: Some(minute),
            ..night()
        };
        assert_eq!(window(&moment), Ok(Window::Night), "{minute}");
    }
    for minute in [0, 59, 5 * 60, 12 * 60, 23 * 60 + 59] {
        let moment = Moment {
            local_minute: Some(minute),
            ..night()
        };
        let why = window(&moment).unwrap_err();
        assert!(
            why.contains("outside the quiet hour at night (01:00 to 05:00)"),
            "{why}"
        );
    }
    let unknown = Moment {
        local_minute: None,
        ..night()
    };
    assert!(
        window(&unknown)
            .unwrap_err()
            .contains("local time is not known")
    );
    let unset = Moment {
        now_unix_ms: 5 * HOUR,
        ..night()
    };
    assert!(window(&unset).is_err(), "a clock not set yet");
}

/// Never in the first round after lamp-update starts, never while the
/// walk is under way, never outside a quiet stretch: whatever the hour.
#[test]
fn the_first_round_a_walk_or_someone_speaking_closes_it() {
    let first = Moment {
        round: 1,
        ..night()
    };
    assert!(window(&first).unwrap_err().contains("first round"));
    assert!(
        window(&Moment {
            round: 0,
            ..night()
        })
        .is_err()
    );
    assert_eq!(
        window(&Moment {
            round: 2,
            ..night()
        }),
        Ok(Window::Night)
    );
    let walking = Moment {
        walk_under_way: true,
        ..night()
    };
    assert_eq!(window(&walking).unwrap_err(), crate::walk::UNDER_WAY);
    let spoken = Moment {
        quiet: Err("someone was heard or answered 30 s ago".into()),
        ..night()
    };
    assert_eq!(
        window(&spoken).unwrap_err(),
        "not a quiet stretch: someone was heard or answered 30 s ago"
    );
}

/// Right after the setup walk ended, its first quiet stretch opens it by
/// day, within six hours and only for the first try since the walk: a try
/// after it (failed or stopped) waits for the night.
#[test]
fn the_first_quiet_stretch_after_the_walk_opens_it_by_day() {
    let day = Moment {
        local_minute: Some(14 * 60),
        walk_ended_unix_ms: Some(NOON - 20 * MIN),
        ..night()
    };
    assert_eq!(window(&day), Ok(Window::AfterWalk));
    let tried_before = Moment {
        tried_unix_ms: Some(NOON - 2 * HOUR),
        ..day.clone()
    };
    assert_eq!(window(&tried_before), Ok(Window::AfterWalk));
    let tried_since = Moment {
        tried_unix_ms: Some(NOON - 5 * MIN),
        ..day.clone()
    };
    assert!(window(&tried_since).is_err());
    assert_eq!(
        window(&Moment {
            local_minute: Some(3 * 60),
            ..tried_since
        }),
        Ok(Window::Night)
    );
    let long_after = Moment {
        walk_ended_unix_ms: Some(NOON - 6 * HOUR),
        ..day.clone()
    };
    assert!(window(&long_after).is_err());
    let ahead = Moment {
        walk_ended_unix_ms: Some(NOON + MIN),
        ..day.clone()
    };
    assert!(window(&ahead).is_err(), "an end after now is not trusted");
    let still_first = Moment {
        round: 1,
        ..day.clone()
    };
    assert!(window(&still_first).is_err());
    let not_quiet = Moment {
        quiet: Err("the voice is speaking".into()),
        ..day
    };
    assert!(window(&not_quiet).is_err());
}

/// A try is one begun or failed: `absent` or `installed` is none.
#[test]
fn a_try_is_an_install_begun_or_failed() {
    let said = |install: &str| HermesInstall {
        install: install.to_owned(),
        revision: 1,
        at_unix_ms: 42,
        version: None,
        commit: None,
        error: None,
    };
    assert_eq!(last_try_unix_ms(None), None);
    assert_eq!(last_try_unix_ms(Some(&said("absent"))), None);
    assert_eq!(last_try_unix_ms(Some(&said("installed"))), None);
    assert_eq!(last_try_unix_ms(Some(&said("installing"))), Some(42));
    assert_eq!(last_try_unix_ms(Some(&said("failed"))), Some(42));
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
/// voice is not quiet and thaws when the quiet stretch is back; each
/// freeze is one act, never repeated.
#[test]
fn someone_heard_freezes_it_and_the_quiet_stretch_thaws_it() {
    let mut watch = Watch::default();
    assert_eq!(watch.look(0, Ok(()), &read("a")), Act::Nothing);
    assert_eq!(
        watch.look(100, NOT_QUIET(), &read("a")),
        Act::Freeze("the voice is hearing someone".into())
    );
    assert!(watch.frozen());
    assert_eq!(watch.look(200, NOT_QUIET(), &read("a")), Act::Nothing);
    assert_eq!(watch.look(11 * MIN, NOT_QUIET(), &read("a")), Act::Nothing);
    assert_eq!(watch.look(12 * MIN, Ok(()), &read("a")), Act::Thaw);
    assert!(!watch.frozen());
    assert_eq!(watch.look(12 * MIN + 100, Ok(()), &read("a")), Act::Nothing);
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

/// Frozen 30 min in all, over one freeze or several, stops it as
/// `interrupted`: never frozen for good, and inside the one-shot's hour.
#[test]
fn frozen_too_long_in_all_stops_it() {
    let mut watch = Watch::default();
    assert!(matches!(
        watch.look(0, NOT_QUIET(), &read("a")),
        Act::Freeze(_)
    ));
    assert_eq!(
        watch.look(30 * MIN - 1, NOT_QUIET(), &read("a")),
        Act::Nothing
    );
    match watch.look(30 * MIN, NOT_QUIET(), &read("a")) {
        Act::Stop { error, why } => {
            assert_eq!(error, "interrupted");
            assert!(why.contains("frozen 30 min in all"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!watch.frozen());
    // Two freezes: 20 min, then 10 more.
    let mut watch = Watch::default();
    assert!(matches!(
        watch.look(0, NOT_QUIET(), &read("a")),
        Act::Freeze(_)
    ));
    assert_eq!(watch.look(20 * MIN, Ok(()), &read("a")), Act::Thaw);
    assert!(matches!(
        watch.look(25 * MIN, NOT_QUIET(), &read("a")),
        Act::Freeze(_)
    ));
    assert_eq!(
        watch.look(35 * MIN - 1, NOT_QUIET(), &read("a")),
        Act::Nothing
    );
    assert!(matches!(
        watch.look(35 * MIN, NOT_QUIET(), &read("a")),
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

/// The quiet stretch is the voice's quiet gate over 10 min: the voice seen
/// resting, nobody heard for 10 min, and 10 min of listening; a voice
/// that says nothing is never quiet.
#[test]
fn the_quiet_stretch_is_ten_minutes_of_the_quiet_gate() {
    let mut quiet = Quiet::new(0);
    assert!(quiet_stretch(&quiet, 11 * MIN).is_err(), "nothing said");
    for at in (0..=11 * MIN).step_by(1000) {
        quiet.hear(Heard::State(Voice::Resting), at);
    }
    assert_eq!(quiet_stretch(&quiet, 11 * MIN), Ok(()));
    quiet.hear(Heard::Speech, 11 * MIN);
    for at in (11 * MIN..=21 * MIN).step_by(1000) {
        quiet.hear(Heard::State(Voice::Resting), at);
    }
    assert!(quiet_stretch(&quiet, 21 * MIN - 1000).is_err());
    assert_eq!(quiet_stretch(&quiet, 21 * MIN), Ok(()));
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

/// The day window (the first builder, 11 Nov 01:15 on #6059): a lamp that
/// never tried, as one that just took the update, installs at its first
/// quiet stretch by day, with or without a local time; still never in the
/// first round, the walk or while someone speaks. Once it tried, by day is
/// closed again and the night stands.
#[test]
fn a_lamp_that_never_tried_installs_at_its_first_quiet_stretch_by_day() {
    let updated = Moment {
        tried_unix_ms: None,
        local_minute: Some(7 * 60 + 20),
        ..night()
    };
    assert_eq!(window(&updated), Ok(Window::AfterUpdate));
    assert_eq!(
        window(&Moment {
            local_minute: None,
            ..updated.clone()
        }),
        Ok(Window::AfterUpdate)
    );
    for closed in [
        Moment {
            round: 1,
            ..updated.clone()
        },
        Moment {
            walk_under_way: true,
            ..updated.clone()
        },
        Moment {
            quiet: Err("the voice is speaking".into()),
            ..updated.clone()
        },
        Moment {
            tried_unix_ms: Some(NOON - HOUR),
            ..updated.clone()
        },
    ] {
        assert!(window(&closed).is_err(), "{closed:?}");
    }
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

/// The app's tap (#6240): past the quiet stretch and the clock windows,
/// by day, with a recent try and with no local time; still never in the
/// first round nor while the walk is under way.
#[test]
fn an_ask_from_the_app_opens_it_outside_the_quiet_stretch_and_the_clock() {
    let asked = Moment {
        asked: true,
        quiet: Err("the voice is speaking".into()),
        tried_unix_ms: Some(NOON - HOUR),
        local_minute: Some(12 * 60),
        ..night()
    };
    assert_eq!(window(&asked), Ok(Window::Asked));
    for open in [
        Moment {
            local_minute: None,
            ..asked.clone()
        },
        Moment {
            now_unix_ms: 5 * HOUR,
            ..asked.clone()
        },
        Moment {
            quiet: Ok(()),
            ..asked.clone()
        },
    ] {
        assert_eq!(window(&open), Ok(Window::Asked), "{open:?}");
    }
    assert!(
        window(&Moment {
            round: 1,
            ..asked.clone()
        })
        .unwrap_err()
        .contains("first round")
    );
    assert_eq!(
        window(&Moment {
            walk_under_way: true,
            ..asked.clone()
        }),
        Err(crate::walk::UNDER_WAY.to_owned())
    );
    // Not asked, the same moment stays closed.
    assert!(
        window(&Moment {
            asked: false,
            ..asked
        })
        .is_err()
    );
    assert_eq!(Window::Asked.name(), "asked from the app");
}
