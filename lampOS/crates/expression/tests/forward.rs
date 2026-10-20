//! #1680, #2052's audit: Expression's catalogue reads one release forward.
//! A catalogue a newer build wrote (a field this build does not know, at
//! any depth) never stops Expression: its fields are read past and named.
//! A known field with a bad value, an unknown value of a known field and
//! another `version` are still refused.
use lamp_expression::Catalogue;
use serde_json::{Value, json};

fn builtin() -> Value {
    serde_json::from_str(include_str!("../behaviours.json")).unwrap()
}

fn read(value: &Value) -> Result<(Catalogue, Vec<String>), String> {
    Catalogue::from_json_naming(&value.to_string()).map_err(|error| format!("{error:?}"))
}

#[test]
fn a_newer_catalogues_fields_are_read_past_and_named_by_path() {
    let mut newer = builtin();
    newer["mood_scale"] = json!(0.5);
    newer["limits"]["neck_twist"] = json!(0.1);
    newer["behaviours"][1]["blend"] = json!("ease");
    newer["behaviours"][1]["keyframes"][0]["curve"] = json!([0.2, 0.8]);
    newer["timing"]["breath_us"] = json!(4_000_000);
    let (catalogue, unread) = read(&newer).unwrap();
    assert_eq!(
        unread,
        [
            "behaviours[1].blend",
            "behaviours[1].keyframes[0].curve",
            "limits.neck_twist",
            "mood_scale",
            "timing.breath_us",
        ]
    );
    // What it knows reads as before.
    assert_eq!(catalogue, Catalogue::builtin());
}

#[test]
fn the_speaking_section_reads_a_newer_key_too() {
    let mut newer: Value = serde_json::from_str(include_str!("../alive-behaviours.json")).unwrap();
    // A later release's speaking key, which this build does not know.
    newer["speaking"]["sway_pitch"] = json!(0.02);
    newer["alive"]["glimmer"] = json!(true);
    let (_, unread) = read(&newer).unwrap();
    assert!(
        unread.contains(&"speaking.sway_pitch".to_owned()),
        "{unread:?}"
    );
    assert!(unread.contains(&"alive.glimmer".to_owned()), "{unread:?}");
}

#[test]
fn a_bad_known_value_an_unknown_trigger_or_another_version_is_still_refused() {
    let mut bad = builtin();
    bad["behaviours"][1]["cooldown_us"] = json!(1);
    assert!(read(&bad).is_err());
    let mut wrong_type = builtin();
    wrong_type["limits"]["base_yaw"] = json!("wide");
    assert!(read(&wrong_type).is_err());
    let mut trigger = builtin();
    trigger["behaviours"][0]["trigger"] = json!("daydreaming");
    assert!(read(&trigger).is_err());
    let mut version = builtin();
    version["version"] = json!(2);
    assert!(read(&version).is_err());
}

/// Every catalogue this build ships names nothing: a field it reads but
/// writes only when set (an empty list, `false`, none) is not taken for
/// one it does not know.
#[test]
fn every_shipped_catalogue_names_no_field_it_does_not_read() {
    for (name, text) in [
        ("behaviours", include_str!("../behaviours.json")),
        ("alive-behaviours", include_str!("../alive-behaviours.json")),
        (
            "playout-behaviours",
            include_str!("../playout-behaviours.json"),
        ),
        ("rest-behaviours", include_str!("../rest-behaviours.json")),
        (
            "product",
            include_str!("../../../deploy/product/settings/expression.json"),
        ),
        (
            "lamp-4ace alive",
            include_str!("../../../deploy/lamp-4ace/expression-alive-lamp-4ace.json"),
        ),
        (
            "lamp-4ace name speech",
            include_str!("../../../deploy/lamp-4ace/expression-alive-name-speech-lamp-4ace.json"),
        ),
        (
            "lamp-4ace rest",
            include_str!("../../../deploy/lamp-4ace/expression-rest-lamp-4ace.json"),
        ),
    ] {
        let (_, unread) = Catalogue::from_json_naming(text).unwrap();
        assert!(unread.is_empty(), "{name}: {unread:?}");
    }
}
