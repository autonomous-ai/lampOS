//! `voice/arrival`, and the arrival a `hello` decision names.
use lamp_bus::Kind;
use lamp_msgs::{Decision, VoiceArrival, arrival_topics, declaration_of, first_topics};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn arrival() -> VoiceArrival {
    VoiceArrival {
        arrival: 98_123_456,
        away_min: 42,
        there_s: 3,
        quiet_s: 310,
        since_hello_min: Some(95),
    }
}

#[test]
fn an_arrival_is_counts_and_times_only_and_ignores_fields_added_later() {
    let text = serde_json::to_string(&arrival()).unwrap();
    assert_eq!(
        text,
        r#"{"arrival":98123456,"away_min":42,"there_s":3,"quiet_s":310,"since_hello_min":95}"#
    );
    assert_eq!(
        serde_json::from_str::<VoiceArrival>(&text).unwrap(),
        arrival()
    );
    let mut later: Value = serde_json::from_str(&text).unwrap();
    later["added_later"] = json!(1);
    assert_eq!(
        serde_json::from_value::<VoiceArrival>(later).unwrap(),
        arrival()
    );
    // Never greeted since the voice started: left out.
    let first = VoiceArrival {
        since_hello_min: None,
        ..arrival()
    };
    assert!(
        !serde_json::to_string(&first)
            .unwrap()
            .contains("since_hello")
    );
    for field in ["arrival", "away_min", "there_s", "quiet_s"] {
        let mut without: Value = serde_json::from_str(&text).unwrap();
        without.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<VoiceArrival>(without).is_err(),
            "{field}"
        );
    }
    arrival().check().unwrap();
    let zero = VoiceArrival {
        arrival: 0,
        ..arrival()
    };
    assert!(zero.check().unwrap_err().0.contains("positive"));
}

#[test]
fn the_topic_is_an_event_apart_from_the_first_topics() {
    let declared = VoiceArrival::declaration();
    assert_eq!(declared.type_name, "lamp_msgs/VoiceArrival");
    assert_eq!((declared.kind, declared.period()), (Kind::Event, None));
    assert_eq!(
        arrival_topics(),
        vec![(VoiceArrival::TOPIC, declared.clone())]
    );
    assert_eq!(declaration_of("voice/arrival"), Some(declared));
    assert!(
        first_topics()
            .iter()
            .all(|(topic, _)| *topic != VoiceArrival::TOPIC)
    );
}

#[test]
fn a_hello_decision_names_its_arrival_and_every_other_decision_is_written_as_before() {
    let attention = Decision {
        about: "attention".into(),
        choice: "answer".into(),
        probability: 0.9,
        answers: BTreeMap::new(),
        by: "rule".into(),
        took_ms: 0,
        shadow: true,
        request: None,
        task: None,
        arrival: None,
        ask: None,
    };
    // The bytes the attention and progress decisions had before the field.
    assert_eq!(
        serde_json::to_string(&attention).unwrap(),
        "{\"about\":\"attention\",\"choice\":\"answer\",\"probability\":0.9,\"answers\":{},\"by\":\"rule\",\"took_ms\":0,\"shadow\":true}"
    );
    let progress = Decision {
        about: "progress".into(),
        choice: "say_now".into(),
        shadow: false,
        task: Some("0123456789abcdef0123456789abcdef:3:4".into()),
        ..attention.clone()
    };
    assert_eq!(
        serde_json::to_string(&progress).unwrap(),
        "{\"about\":\"progress\",\"choice\":\"say_now\",\"probability\":0.9,\"answers\":{},\"by\":\"rule\",\"took_ms\":0,\"shadow\":false,\"task\":\"0123456789abcdef0123456789abcdef:3:4\"}"
    );
    let hello = Decision {
        about: "hello".into(),
        choice: "wait".into(),
        arrival: Some(98_123_456),
        ..attention
    };
    hello.check().unwrap();
    let text = serde_json::to_string(&hello).unwrap();
    assert!(
        text.ends_with(",\"shadow\":true,\"arrival\":98123456}"),
        "{text}"
    );
    assert_eq!(serde_json::from_str::<Decision>(&text).unwrap(), hello);
}
