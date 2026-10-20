//! `skills/events`: what a physical skill did and why.
use lamp_bus::Kind;
use lamp_msgs::{SkillEvent, SkillStage, declaration_of, skills_topics};
use serde_json::{Value, json};

fn answered() -> SkillEvent {
    SkillEvent {
        skill: "welcome-back".into(),
        stage: SkillStage::Answered,
        why: "policy hello: the ask is skipped".into(),
        allowed: Some(true),
    }
}

#[test]
fn an_event_is_written_as_intended_and_ignores_fields_added_later() {
    let event = answered();
    event.check().unwrap();
    let text = serde_json::to_string(&event).unwrap();
    assert_eq!(
        text,
        r#"{"skill":"welcome-back","stage":"answered","why":"policy hello: the ask is skipped","allowed":true}"#
    );
    let mut later: Value = serde_json::from_str(&text).unwrap();
    later["future"] = json!(1);
    assert_eq!(serde_json::from_value::<SkillEvent>(later).unwrap(), event);
    for field in ["skill", "stage", "why"] {
        let mut without: Value = serde_json::from_str(&text).unwrap();
        without.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<SkillEvent>(without).is_err(),
            "{field}"
        );
    }
    let mut unknown: Value = serde_json::from_str(&text).unwrap();
    unknown["stage"] = json!("dreamt");
    assert!(serde_json::from_value::<SkillEvent>(unknown).is_err());
}

#[test]
fn the_check_holds_the_name_and_allowed_to_answered() {
    let mut event = answered();
    event.skill = "Welcome Back".into();
    assert!(event.check().unwrap_err().0.contains("lowercase"));
    let mut event = answered();
    event.allowed = None;
    assert!(event.check().unwrap_err().0.contains("allowed"));
    let mut event = answered();
    event.stage = SkillStage::Done;
    assert!(event.check().unwrap_err().0.contains("allowed"));
    event.allowed = None;
    event.check().unwrap();
}

#[test]
fn the_topic_is_an_event_found_by_its_name() {
    let declared = declaration_of(SkillEvent::TOPIC).unwrap();
    assert_eq!(declared.type_name, SkillEvent::TYPE);
    assert_eq!(declared.kind, Kind::Event);
    assert_eq!(skills_topics().len(), 1);
}
