use lamp_memory_bench::{
    corpus::{self, Persona},
    files,
    protocol::Event,
    score::Answer,
};
fn persona() -> Persona {
    serde_json::from_str(include_str!(
        "../../../fixtures/memory-bench/development/persona.json"
    ))
    .unwrap()
}
#[test]
fn fictional_month_is_repeatable_and_questions_never_cite_future_records() {
    let a = corpus::generate(persona()).unwrap();
    let b = corpus::generate(persona()).unwrap();
    files::validate(&a).unwrap();
    assert_eq!(a.events.len(), 1560);
    assert_eq!(a.questions.len(), 200);
    assert_eq!(
        serde_json::to_vec(&a).unwrap(),
        serde_json::to_vec(&b).unwrap()
    );
    assert_eq!(corpus::counts(&a).get("ahead"), Some(&12));
    assert!(
        a.questions
            .iter()
            .filter(|q| q.category == "ahead")
            .all(|q| q.followups.len() == 2)
    );
    let mut modified = a;
    modified.questions[0].evidence_ids.push("d30-050".into());
    assert!(files::validate(&modified).is_err());
}
#[test]
fn local_days_follow_dst_instead_of_adding_twenty_four_hours() {
    let mut p = persona();
    p.timezone = "America/New_York".into();
    p.start_date = "2030-03-01".into();
    assert_eq!(
        p.at(10, 9, 0).unwrap() - p.at(9, 9, 0).unwrap(),
        23 * 60 * 60 * 1000
    );
    assert_eq!(p.local_date(10).unwrap().to_string(), "2030-03-10");
}
#[test]
fn event_input_cannot_smuggle_an_answer_key() {
    let c = corpus::generate(persona()).unwrap();
    let mut value = serde_json::to_value(&c.events[1]).unwrap();
    value["true_speaker"] = serde_json::json!("owner-a");
    assert!(serde_json::from_value::<Event>(value).is_err());
}
#[test]
fn an_unbooked_conditional_plan_accepts_no_not_undecided() {
    let c = corpus::generate(persona()).unwrap();
    let q = c
        .questions
        .iter()
        .find(|q| q.query.heard.starts_with("Have I booked"))
        .unwrap();
    let a = Answer {
        answer: "no".into(),
        evidence_ids: vec![],
        abstain: false,
    };
    assert_eq!(q.score(&a).unwrap().fraction, Some(1.0));
}
#[test]
fn rejecting_a_bad_recommendation_is_not_a_literal_disclosure() {
    let c = corpus::generate(persona()).unwrap();
    let q = c
        .questions
        .iter()
        .find(|q| q.category == "understanding" && q.query.heard.contains("on a call"))
        .unwrap();
    let a = Answer {
        answer: "I will not interrupt the call.".into(),
        evidence_ids: vec![],
        abstain: false,
    };
    assert!(q.score(&a).unwrap().needs_judge);
}
#[test]
fn unsupported_truth_timelines_and_colliding_deletions_are_refused() {
    let mut p = persona();
    p.facts[0].revision_day = Some(25);
    assert!(p.validate().is_err());
    let mut p = persona();
    p.facts[38].forget_day = None;
    p.facts[36].forget_day = Some(11);
    p.facts[39].forget_day = None;
    p.facts[38].forget_day = Some(11);
    assert!(p.validate().is_err());
}
