//! `room/activity` and `room/stood_up` (#1678): what the face at the desk
//! shows, and someone standing up to leave.
use lamp_bus::Kind;
use lamp_msgs::{Activity, RoomActivity, StoodUp, activity_topics, declaration_of, first_topics};
use std::time::Duration;

#[test]
fn activity_is_a_state_and_three_counts() {
    let reading = RoomActivity {
        state: Activity::HeadDown,
        for_s: 340,
        sitting_s: 2710,
        baseline: true,
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
    for state in [
        "away",
        "at_desk",
        "leaning_in",
        "leaning_back",
        "head_down",
        "sitting_low",
    ] {
        let text = format!(r#"{{"state":"{state}","for_s":1,"sitting_s":0,"baseline":true}}"#);
        assert!(
            serde_json::from_str::<RoomActivity>(&text).is_ok(),
            "{state}"
        );
    }
    assert!(
        serde_json::from_str::<RoomActivity>(
            r#"{"state":"slumped","for_s":1,"sitting_s":1,"baseline":true}"#
        )
        .is_err()
    );
    // Away has no sitting; leaning or lower needs the baseline.
    let away = RoomActivity {
        state: Activity::Away,
        for_s: 30,
        sitting_s: 0,
        baseline: false,
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

#[test]
fn activity_is_kept_and_standing_up_is_an_event_apart_from_the_first_topics() {
    let kept = RoomActivity::declaration();
    assert_eq!(kept.kind, Kind::Latest);
    assert_eq!(kept.period_ms, Some(1_000));
    assert_eq!(kept.lifespan_ms, Some(3_000));
    assert_eq!(RoomActivity::LASTS, Duration::from_secs(3));
    assert_eq!(StoodUp::declaration().kind, Kind::Event);
    let names: Vec<&str> = activity_topics().iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["room/activity", "room/stood_up"]);
    for name in names {
        assert!(declaration_of(name).is_some(), "{name}");
        assert!(
            !first_topics().iter().any(|(first, _)| *first == name),
            "{name}"
        );
    }
    assert_eq!(
        serde_json::to_string(&StoodUp { sitting_s: 3000 }).unwrap(),
        r#"{"sitting_s":3000}"#
    );
}
