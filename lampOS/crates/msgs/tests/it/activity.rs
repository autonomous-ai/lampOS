//! `room/activity` and `room/stood_up` (#1678): what the face at the desk
//! shows, and someone standing up to leave.
use lamp_bus::Kind;
use lamp_msgs::{
    Activity, RoomActivity, Side, StoodUp, activity_topics, declaration_of, first_topics,
};
use std::time::Duration;

#[test]
fn activity_is_a_state_and_three_counts() {
    let reading = RoomActivity {
        state: Activity::HeadDown,
        for_s: 340,
        sitting_s: 2710,
        baseline: true,
        yawns: 0,
    };
    let text = serde_json::to_string(&reading).unwrap();
    assert_eq!(
        text,
        r#"{"state":"head_down","for_s":340,"sitting_s":2710,"baseline":true}"#
    );
    assert_eq!(
        serde_json::from_str::<RoomActivity>(&text).unwrap(),
        reading
    );
    reading.check().unwrap();
    for state in ["away", "at_desk", "leaning_in", "head_down"] {
        let text = format!(r#"{{"state":"{state}","for_s":1,"sitting_s":0,"baseline":true}}"#);
        assert!(
            serde_json::from_str::<RoomActivity>(&text).is_ok(),
            "{state}"
        );
    }
    // Not states (leaning back and sitting low were dropped, #1678).
    for state in ["slumped", "leaning_back", "sitting_low"] {
        let text = format!(r#"{{"state":"{state}","for_s":1,"sitting_s":1,"baseline":true}}"#);
        assert!(
            serde_json::from_str::<RoomActivity>(&text).is_err(),
            "{state}"
        );
    }
    // Away has no sitting; leaning in or head down needs the baseline.
    let away = RoomActivity {
        state: Activity::Away,
        for_s: 30,
        sitting_s: 0,
        baseline: false,
        yawns: 0,
    };
    away.check().unwrap();
    assert!(
        RoomActivity {
            sitting_s: 5,
            ..away
        }
        .check()
        .is_err()
    );
    assert!(
        RoomActivity {
            baseline: false,
            ..reading
        }
        .check()
        .is_err()
    );
    RoomActivity {
        state: Activity::AtDesk,
        baseline: false,
        ..reading
    }
    .check()
    .unwrap();
}

/// A sitting's yawns (#3709): said only once there is one, so an eyes that
/// counts none says what it said before, and an old eyes' line reads as 0;
/// away has none.
#[test]
fn yawns_are_a_count_said_from_the_first() {
    let sat = RoomActivity {
        state: Activity::AtDesk,
        for_s: 60,
        sitting_s: 1800,
        baseline: true,
        yawns: 2,
    };
    let text = serde_json::to_string(&sat).unwrap();
    assert_eq!(
        text,
        r#"{"state":"at_desk","for_s":60,"sitting_s":1800,"baseline":true,"yawns":2}"#
    );
    assert_eq!(serde_json::from_str::<RoomActivity>(&text).unwrap(), sat);
    sat.check().unwrap();
    let old = r#"{"state":"at_desk","for_s":60,"sitting_s":1800,"baseline":true}"#;
    assert_eq!(
        serde_json::from_str::<RoomActivity>(old).unwrap(),
        RoomActivity { yawns: 0, ..sat }
    );
    assert_eq!(
        serde_json::to_string(&RoomActivity { yawns: 0, ..sat }).unwrap(),
        old
    );
    assert!(
        RoomActivity {
            state: Activity::Away,
            sitting_s: 0,
            baseline: false,
            ..sat
        }
        .check()
        .is_err()
    );
}

#[test]
fn activity_is_kept_and_standing_up_is_an_event_apart_from_the_first_topics() {
    let kept = RoomActivity::declaration();
    assert_eq!(kept.kind, Kind::Latest);
    assert_eq!(kept.period_ms, Some(1_000));
    assert_eq!(kept.lifespan_ms, Some(3_000));
    assert_eq!(RoomActivity::LASTS, Duration::from_secs(3));
    assert_eq!(StoodUp::declaration().kind, Kind::Event);
    assert_eq!(StoodUp::declaration().lifespan_ms, Some(5_000));
    let names: Vec<&str> = activity_topics().iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["room/activity", "room/stood_up"]);
    for name in names {
        assert!(declaration_of(name).is_some(), "{name}");
        assert!(
            !first_topics().iter().any(|(first, _)| *first == name),
            "{name}"
        );
    }
    // No side, near the middle: not written, so a reader before it sees the
    // message as it was; one written before it reads as no side.
    assert_eq!(
        serde_json::to_string(&StoodUp {
            sitting_s: 3000,
            side: None
        })
        .unwrap(),
        r#"{"sitting_s":3000}"#
    );
    assert_eq!(
        serde_json::from_str::<StoodUp>(r#"{"sitting_s":3000}"#).unwrap(),
        StoodUp {
            sitting_s: 3000,
            side: None
        }
    );
    assert_eq!(
        serde_json::to_string(&StoodUp {
            sitting_s: 60,
            side: Some(Side::Left)
        })
        .unwrap(),
        r#"{"sitting_s":60,"side":"left"}"#
    );
}
