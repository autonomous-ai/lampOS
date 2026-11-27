use lamp_apps::facts::{APP_GRANTS, DOOR_GRANTS, check, unit_of};
use lamp_apps::testing::StubApp;

#[test]
fn a_good_app_passes_and_net_picks_the_unit() {
    let mut app = StubApp::good("english-lessons");
    assert_eq!(check(&app, Some(200)), Ok(()));
    assert_eq!(unit_of(&app), "lamp-app@english-lessons.service");
    app.net = true;
    assert_eq!(unit_of(&app), "lamp-app-net@english-lessons.service");
}

#[test]
fn net_is_the_top_level_field_not_a_grant() {
    let mut app = StubApp::good("x");
    app.grants = vec!["net".into()];
    assert!(check(&app, Some(200)).is_err(), "net is not a grant name");
    app.grants = vec!["logs".into()];
    assert!(check(&app, Some(200)).is_err(), "logs is not a grant name");
}

#[test]
fn only_the_doors_plain_grants_and_no_service_grant() {
    let mut app = StubApp::good("x");
    for grant in DOOR_GRANTS {
        app.grants = vec![(*grant).to_owned()];
        assert_eq!(check(&app, Some(200)), Ok(()), "{grant}");
    }
    for bad in ["calendar-read", "mail", "x", "", "Body"] {
        app.grants = vec![bad.to_owned()];
        assert!(check(&app, Some(200)).is_err(), "{bad}");
    }
    app.grants = vec!["body".into(), "body".into()];
    assert!(check(&app, Some(200)).is_err(), "a grant twice");
    // An app's own grant (`remember`, apps.remember), beside the door's.
    assert_eq!(APP_GRANTS, ["remember", "turns", "report"]);
    app.grants = vec!["remember".into(), "speak".into()];
    assert_eq!(check(&app, Some(200)), Ok(()));
    app.grants = vec!["remember".into(), "remember".into()];
    assert!(check(&app, Some(200)).is_err(), "remember twice");
}

#[test]
fn min_build_refuses_when_the_build_is_unknown_or_lower() {
    let mut app = StubApp::good("x");
    app.min_build = 143;
    assert!(
        check(&app, None).is_err(),
        "no lamp.build yet: refuse, never pass"
    );
    assert!(check(&app, Some(142)).is_err());
    assert_eq!(check(&app, Some(143)), Ok(()));
}

#[test]
fn bad_name_entry_version_and_title_are_refused() {
    let mut app = StubApp::good("x");
    app.name = "../x".into();
    assert!(check(&app, Some(200)).is_err());
    let mut app = StubApp::good("x");
    app.entry = "a/b".into();
    assert!(check(&app, Some(200)).is_err());
    for bad in ["1.2", "1.2.3.4", "a.b.c", "1.2.", "", "1.2.-3", "1..3"] {
        let mut app = StubApp::good("x");
        app.version = bad.into();
        assert!(check(&app, Some(200)).is_err(), "{bad}");
    }
    for bad in ["", "a\nb", &"t".repeat(41)] {
        let mut app = StubApp::good("x");
        app.title = bad.to_owned();
        assert!(check(&app, Some(200)).is_err(), "{bad:?}");
    }
}

#[test]
fn the_door_list_matches_lamp_controls_own() {
    // lamp-control is not a dependency (it is a server); the list is copied,
    // so a drift is caught here.
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../control/src/protocol.rs"
    ))
    .unwrap();
    let from = source
        .find("const PLAIN_WIRE: [&str; 15] = [")
        .expect("PLAIN_WIRE");
    let block = &source[from..];
    let block = &block[..block.find("];").unwrap()];
    let theirs: Vec<&str> = block
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"')?.strip_suffix("\","))
        .collect();
    assert_eq!(theirs, DOOR_GRANTS);
}

#[test]
fn an_app_naming_lamp_doctors_turns_and_report_passes_with_and_without_net() {
    // The installer refuses a grant it does not know; the manifest reader
    // (`lamp_skills::app_package::GRANTS`) takes these two behind their
    // switches, so it must too, and off it refuses them as the reader does.
    lamp_skills::app_package::with_doctor_switches(true, true, || {
        for net in [false, true] {
            let mut app = StubApp::good("lamp-doctor");
            app.net = net;
            app.grants = vec!["turns".into(), "report".into(), "device".into()];
            assert_eq!(check(&app, Some(200)), Ok(()), "net {net}");
        }
        let mut app = StubApp::good("lamp-doctor");
        app.grants = vec!["turns".into(), "turns".into()];
        assert!(check(&app, Some(200)).is_err(), "turns twice");
        // Not this version's: they are refused until they are built.
        for later in ["conversation", "model"] {
            app.grants = vec![later.into()];
            assert!(check(&app, Some(200)).is_err(), "{later}");
        }
    });
    let mut app = StubApp::good("lamp-doctor");
    for grant in ["turns", "report"] {
        app.grants = vec![grant.into()];
        assert!(
            check(&app, Some(200)).is_err(),
            "{grant} with its switch off"
        );
    }
    lamp_skills::app_package::with_doctor_switches(true, false, || {
        app.grants = vec!["turns".into()];
        assert_eq!(check(&app, Some(200)), Ok(()));
        app.grants = vec!["report".into()];
        assert!(check(&app, Some(200)).is_err());
    });
}

#[test]
fn the_installers_grants_are_the_manifest_readers() {
    let mut installer: Vec<&str> = DOOR_GRANTS.iter().chain(APP_GRANTS).copied().collect();
    let mut reader: Vec<&str> = lamp_skills::app_package::GRANTS.to_vec();
    installer.sort_unstable();
    reader.sort_unstable();
    assert_eq!(installer, reader);
}

#[test]
fn report_is_refused_beside_the_grants_that_read_what_a_person_says_or_does() {
    lamp_skills::app_package::with_doctor_switches(true, true, || {
        let mut app = StubApp::good("lamp-doctor");
        for reader in [
            "hear",
            "speech",
            "microphone",
            "camera",
            "notes",
            "remember",
        ] {
            app.grants = vec!["report".into(), reader.into()];
            let why = check(&app, Some(200)).unwrap_err();
            assert!(
                why.contains("report can't be held beside"),
                "{reader}: {why}"
            );
        }
        // `turns` has no such rule, and `report` beside `device` is right.
        app.grants = vec!["turns".into(), "hear".into()];
        assert_eq!(check(&app, Some(200)), Ok(()));
        app.grants = vec!["report".into(), "device".into()];
        assert_eq!(check(&app, Some(200)), Ok(()));
    });
}
