//! `skills/events`: what a physical skill did and why.
use lamp_bus::Kind;
use lamp_msgs::{SkillEvent, SkillStage, SkillTask, SkillTaskStage, declaration_of, skills_topics};
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
    assert_eq!(skills_topics().len(), 2);
    let declared = declaration_of(SkillTask::TOPIC).unwrap();
    assert_eq!(declared.type_name, SkillTask::TYPE);
    assert_eq!(declared.kind, Kind::Event);
}

fn news(stage: SkillTaskStage) -> SkillTask {
    use SkillTaskStage::{Failed, Held, Ran, Started};
    SkillTask {
        skill: "mail-check".into(),
        stage,
        run: matches!(stage, Started | Ran | Failed)
            .then_some("task-1795056147468-853891-0".into()),
        engine: (stage == Started).then_some("openclaw".into()),
        gate: (stage == Held).then_some("not alone".into()),
    }
}

/// #7873: a skill's run and its answer at its gate, by name, id, engine
/// and the gate's own reason: each field with its stages only.
#[test]
fn a_skill_task_carries_its_run_engine_and_gate_with_their_stages_only() {
    use SkillTaskStage::{Dropped, Failed, Held, Ran, Started, Told};
    for stage in [Started, Ran, Failed, Held, Told, Dropped] {
        news(stage).check().unwrap();
    }
    assert_eq!(
        serde_json::to_string(&news(Started)).unwrap(),
        r#"{"skill":"mail-check","stage":"started","run":"task-1795056147468-853891-0","engine":"openclaw"}"#
    );
    assert_eq!(
        serde_json::to_string(&news(Held)).unwrap(),
        r#"{"skill":"mail-check","stage":"held","gate":"not alone"}"#
    );
    let mut later: Value = serde_json::to_value(news(Told)).unwrap();
    later["future"] = json!(1);
    assert_eq!(
        serde_json::from_value::<SkillTask>(later).unwrap(),
        news(Told)
    );
    let mut wrong = news(Ran);
    wrong.engine = Some("openclaw".into());
    assert!(wrong.check().unwrap_err().0.contains("engine"));
    let mut wrong = news(Told);
    wrong.run = Some("task-1".into());
    assert!(wrong.check().unwrap_err().0.contains("run"));
    let mut wrong = news(Held);
    wrong.gate = None;
    assert!(wrong.check().unwrap_err().0.contains("gate"));
    let mut wrong = news(Held);
    wrong.gate = Some("She said: <b>hi</b>".into());
    assert!(wrong.check().unwrap_err().0.contains("gate"));
    let mut wrong = news(Started);
    wrong.run = Some("task 1; rm".into());
    assert!(wrong.check().unwrap_err().0.contains("run"));
    let mut wrong = news(Started);
    wrong.skill = "Mail Check".into();
    assert!(wrong.check().unwrap_err().0.contains("skill"));
}
