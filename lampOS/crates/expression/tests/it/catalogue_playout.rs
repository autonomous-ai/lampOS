use lamp_expression::Catalogue;

#[test]
fn existing_catalogues_leave_recent_playout_disabled() {
    assert_eq!(Catalogue::builtin().timing.recent_playout_us, 0);
    let rest = Catalogue::from_json(include_str!("../../rest-behaviours.json")).unwrap();
    assert_eq!(rest.timing.recent_playout_us, 0);
}

#[test]
fn recent_playout_catalogue_changes_only_the_explicit_timing_option() {
    let mut expected = Catalogue::builtin();
    expected.timing.recent_playout_us = 100_000;
    let actual = Catalogue::from_json(include_str!("../../playout-behaviours.json")).unwrap();
    assert_eq!(actual, expected);
    assert!(!actual.resumable_voice);
    assert_eq!(actual.timing.body_lag_us, None);
}

#[test]
fn recent_playout_cannot_be_shorter_than_a_tick_or_exceed_input_lifespan() {
    for value in [0, 20_000, 100_000, 200_000] {
        let mut catalogue = Catalogue::builtin();
        catalogue.timing.recent_playout_us = value;
        catalogue.check().unwrap();
    }
    for value in [1, 19_999, 200_001, u64::MAX] {
        let mut catalogue = Catalogue::builtin();
        catalogue.timing.recent_playout_us = value;
        assert!(
            catalogue
                .check()
                .unwrap_err()
                .to_string()
                .contains("recent_playout_us"),
            "invalid duration {value} was not identified"
        );
    }
}

#[test]
fn recent_playout_is_an_independent_option_for_the_resumable_protocol() {
    let mut catalogue = Catalogue::from_json(include_str!("../../rest-behaviours.json")).unwrap();
    catalogue.timing.recent_playout_us = 100_000;
    catalogue.check().unwrap();
    let json = serde_json::to_string(&catalogue).unwrap();
    assert_eq!(Catalogue::from_json(&json).unwrap(), catalogue);
}

#[test]
fn recent_playout_requires_an_integer_duration() {
    for value in ["-1", "null", "true", "1.5", "\"100000\""] {
        let text = include_str!("../../playout-behaviours.json").replace(
            "\"recent_playout_us\": 100000",
            &format!("\"recent_playout_us\": {value}"),
        );
        assert!(Catalogue::from_json(&text).is_err(), "accepted {value}");
    }
}
