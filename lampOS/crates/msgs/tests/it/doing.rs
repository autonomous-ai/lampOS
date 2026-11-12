//! `humans/doing`, `humans/doing_seen` and `humans/posture` (#6543): what
//! the person at the desk was seen doing and how they sat, from pictures
//! sent to be labelled. Declared now; `lamp-seen` publishes them later.
use lamp_bus::Kind;
use lamp_msgs::{DoingSeen, HumanDoing, HumanPosture, declaration_of, doing_topics, first_topics};
use std::time::Duration;

#[test]
fn doing_and_posture_are_kept_and_a_sighting_is_an_event_apart_from_the_first_topics() {
    for kept in [HumanDoing::declaration(), HumanPosture::declaration()] {
        assert_eq!(kept.kind, Kind::Latest);
        assert_eq!(kept.period_ms, Some(30_000));
        assert_eq!(kept.lifespan_ms, Some(90_000));
        assert_eq!(kept.version, 1);
    }
    assert_eq!(HumanDoing::LASTS, Duration::from_secs(90));
    assert_eq!(HumanDoing::declaration().type_name, "lamp_msgs/HumanDoing");
    assert_eq!(
        HumanPosture::declaration().type_name,
        "lamp_msgs/HumanPosture"
    );
    let seen = DoingSeen::declaration();
    assert_eq!(seen.kind, Kind::Event);
    assert_eq!(seen.period_ms, None);
    assert_eq!(seen.lifespan_ms, Some(5_000));
    let names: Vec<&str> = doing_topics().iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        ["humans/doing", "humans/doing_seen", "humans/posture"]
    );
    for (name, declared) in doing_topics() {
        assert_eq!(declaration_of(name), Some(declared), "{name}");
        assert!(
            !first_topics().iter().any(|(first, _)| *first == name),
            "{name}"
        );
    }
}
