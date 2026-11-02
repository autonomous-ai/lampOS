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
    assert_eq!(a.events.len(), 2220);
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
fn conversations_are_bursts_with_arrivals_departures_and_daily_sleep_boundaries() {
    use lamp_memory_bench::protocol::EventKind;
    let c = corpus::generate(persona()).unwrap();
    let mut lengths = std::collections::BTreeMap::<String, Vec<u64>>::new();
    for e in &c.events {
        if e.kind == EventKind::Utterance {
            assert!(["Got it.", "Okay.", "I hear you."].contains(&e.said.as_str()));
            lengths
                .entry(e.session_id.clone())
                .or_default()
                .push(e.at_unix_ms);
        }
    }
    assert_eq!(lengths.len(), 240);
    for times in lengths.values() {
        assert!((2..=12).contains(&times.len()));
        assert!(
            times
                .windows(2)
                .all(|w| (3_000..=30_000).contains(&(w[1] - w[0])))
        );
    }
    for kind in [EventKind::Arrive, EventKind::Leave] {
        assert_eq!(c.events.iter().filter(|e| e.kind == kind).count(), 120);
    }
    assert_eq!(c.day_end_records().unwrap().len(), 30);
    for day in 1..=30 {
        let groups = lengths
            .iter()
            .filter(|(id, _)| id.starts_with(&format!("d{day:02}-")))
            .map(|(_, v)| v)
            .collect::<Vec<_>>();
        assert!(
            groups
                .windows(2)
                .all(|w| w[1][0] - w[0].last().unwrap() > 600_000)
        );
    }
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
    p.seed = u64::MAX;
    let c = corpus::generate(p).unwrap();
    files::validate(&c).unwrap();
    assert_eq!(c.day_end_records().unwrap().len(), 30);
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
#[test]
fn later_query_time_cannot_authorize_sources_after_the_ingestion_boundary() {
    let mut c = corpus::generate(persona()).unwrap();
    c.questions[0].after_record = c.events[0].record_id.clone();
    assert!(files::validate(&c).is_err());
}
#[test]
fn a_second_resident_is_observed_as_owner_without_revealing_the_true_speaker() {
    let c = corpus::generate(persona()).unwrap();
    for id in ["d08-020", "d08-021", "d08-022"] {
        let event = c.events.iter().find(|e| e.record_id == id).unwrap();
        assert_eq!(event.person, lamp_memory_bench::protocol::Person::Owner);
        assert_eq!(
            event.presence,
            lamp_memory_bench::protocol::Presence::OwnerAlone
        );
        assert_eq!(event.person_id.as_deref(), Some(c.persona.id.as_str()));
        assert!(!serde_json::to_string(event).unwrap().contains("resident-b"));
        assert_eq!(
            c.answer_key
                .iter()
                .find(|e| e.record_id == id)
                .unwrap()
                .true_speaker,
            "resident-b"
        );
    }
    assert_eq!(
        c.questions
            .iter()
            .filter(|q| q.category == "wrong_person_retention" && q.human_audit)
            .count(),
        3
    );
    files::validate(&c).unwrap();
}

#[test]
fn forgetting_persistence_controls_are_in_the_main_stream_before_every_probe() {
    let c = corpus::generate(persona()).unwrap();
    let controls = c
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.heard.starts_with("Please forget my "))
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 2);
    for q in c.questions.iter().filter(|q| q.category == "forgetting") {
        let prefix = c
            .events
            .iter()
            .position(|e| e.record_id == q.after_record)
            .unwrap();
        assert!(controls.iter().all(|i| *i < prefix));
        assert!(!q.query.heard.starts_with("Please forget"));
        assert!(q.followups.is_empty());
    }
}

#[test]
fn cues_are_only_scripted_seat_states_and_memory_queries_use_four_kibibytes() {
    let c = corpus::generate(persona()).unwrap();
    let states = c
        .events
        .iter()
        .map(|e| e.cues.activity.as_deref().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        states,
        std::collections::BTreeSet::from(["away", "at_desk", "head_down"])
    );
    for e in &c.events {
        assert!(e.cues.topic.is_none() && e.cues.objects.is_empty());
        if e.kind == lamp_memory_bench::protocol::EventKind::Leave {
            assert_eq!(e.cues.activity.as_deref(), Some("away"));
        }
    }
    for q in c
        .questions
        .iter()
        .flat_map(|q| std::iter::once(&q.query).chain(&q.followups))
    {
        assert_eq!(q.max_context_bytes, 4096);
        assert!(matches!(
            q.cues.activity.as_deref(),
            Some("at_desk" | "head_down")
        ));
        assert!(q.cues.topic.is_none() && q.cues.objects.is_empty());
    }
}
