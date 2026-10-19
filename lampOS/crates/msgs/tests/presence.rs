//! `room/presence` (#1677): someone at the lamp, or nobody for how long.
use lamp_bus::Kind;
use lamp_msgs::{RoomPresence, declaration_of, first_topics, presence_topics};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn presence_is_two_numbers_and_ignores_fields_added_later() {
    let away = RoomPresence {
        someone: false,
        nobody_s: 312,
    };
    let text = serde_json::to_string(&away).unwrap();
    assert_eq!(text, r#"{"someone":false,"nobody_s":312}"#);
    assert_eq!(serde_json::from_str::<RoomPresence>(&text).unwrap(), away);
    let mut later: Value = serde_json::from_str(&text).unwrap();
    later["added_later"] = json!(1);
    assert_eq!(serde_json::from_value::<RoomPresence>(later).unwrap(), away);
    for field in ["someone", "nobody_s"] {
        let mut without: Value = serde_json::from_str(&text).unwrap();
        without.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<RoomPresence>(without).is_err(),
            "{field}"
        );
    }
    away.check().unwrap();
    // Nobody since the eyes began looking a moment ago.
    RoomPresence {
        someone: false,
        nobody_s: 0,
    }
    .check()
    .unwrap();
    RoomPresence {
        someone: true,
        nobody_s: 0,
    }
    .check()
    .unwrap();
    let both = RoomPresence {
        someone: true,
        nobody_s: 4,
    };
    assert!(both.check().unwrap_err().0.contains("0 while someone is"));
}

#[test]
fn the_topic_is_kept_said_each_second_and_good_for_three_apart_from_the_first_topics() {
    let declared = RoomPresence::declaration();
    assert_eq!(declared.type_name, "lamp_msgs/RoomPresence");
    assert_eq!(
        (declared.kind, declared.period(), declared.lifespan()),
        (
            Kind::Latest,
            Some(Duration::from_secs(1)),
            Some(Duration::from_secs(3))
        )
    );
    assert_eq!(
        presence_topics(),
        vec![(RoomPresence::TOPIC, declared.clone())]
    );
    assert_eq!(declaration_of("room/presence"), Some(declared));
    assert!(
        first_topics()
            .iter()
            .all(|(topic, _)| *topic != RoomPresence::TOPIC)
    );
}
