//! `board/heat` (#7879): how hot the board is for the work that can wait.
use lamp_bus::Kind;
use lamp_msgs::{BoardHeat, HeatLevel, declaration_of, first_topics, heat_topics};
use serde_json::{Value, json};
use std::time::Duration;

fn warm() -> BoardHeat {
    BoardHeat {
        level: HeatLevel::Warm,
        zone: "cpub_thermal_zone".into(),
        c: 85.5,
        trip_c: 90.0,
    }
}

#[test]
fn the_heat_is_a_level_a_zone_and_two_temperatures_and_ignores_fields_added_later() {
    let heat = warm();
    let text = serde_json::to_string(&heat).unwrap();
    assert_eq!(
        text,
        r#"{"level":"warm","zone":"cpub_thermal_zone","c":85.5,"trip_c":90.0}"#
    );
    assert_eq!(serde_json::from_str::<BoardHeat>(&text).unwrap(), heat);
    let mut later: Value = serde_json::from_str(&text).unwrap();
    later["added_later"] = json!(1);
    assert_eq!(serde_json::from_value::<BoardHeat>(later).unwrap(), heat);
    for field in ["level", "zone", "c", "trip_c"] {
        let mut without: Value = serde_json::from_str(&text).unwrap();
        without.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<BoardHeat>(without).is_err(),
            "{field}"
        );
    }
    for (level, word) in [
        (HeatLevel::Ok, "ok"),
        (HeatLevel::Warm, "warm"),
        (HeatLevel::Hot, "hot"),
    ] {
        assert_eq!(serde_json::to_value(level).unwrap(), json!(word));
        assert_eq!(level.as_str(), word);
    }
    // A level this version does not know is no heat: not guessed.
    let mut boiling: Value = serde_json::from_str(&text).unwrap();
    boiling["level"] = json!("boiling");
    assert!(serde_json::from_value::<BoardHeat>(boiling).is_err());
}

#[test]
fn a_heat_names_its_zone_and_its_temperatures_are_numbers() {
    warm().check().unwrap();
    BoardHeat {
        zone: "z".repeat(BoardHeat::MOST_ZONE_BYTES),
        ..warm()
    }
    .check()
    .unwrap();
    for bad in [
        BoardHeat {
            zone: String::new(),
            ..warm()
        },
        BoardHeat {
            zone: "z".repeat(BoardHeat::MOST_ZONE_BYTES + 1),
            ..warm()
        },
        BoardHeat {
            c: f32::NAN,
            ..warm()
        },
        BoardHeat {
            trip_c: f32::INFINITY,
            ..warm()
        },
        BoardHeat {
            trip_c: 0.0,
            ..warm()
        },
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
}

#[test]
fn the_topic_is_kept_said_every_five_seconds_and_good_for_fifteen_apart_from_the_first_topics() {
    let declared = BoardHeat::declaration();
    assert_eq!(declared.type_name, "lamp_msgs/BoardHeat");
    assert_eq!(declared.version, 1);
    assert_eq!(
        (declared.kind, declared.period(), declared.lifespan()),
        (
            Kind::Latest,
            Some(Duration::from_secs(5)),
            Some(Duration::from_secs(15))
        )
    );
    assert_eq!(heat_topics(), vec![("board/heat", declared.clone())]);
    assert_eq!(declaration_of("board/heat"), Some(declared));
    assert!(
        first_topics()
            .iter()
            .all(|(topic, _)| *topic != BoardHeat::TOPIC)
    );
}
