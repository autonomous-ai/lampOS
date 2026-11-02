use lamp_memory_bench::{
    aging::{self, Scenario},
    corpus::{self, Persona, Profile},
    files, hash,
    protocol::EventKind,
};
fn fixture() -> (lamp_memory_bench::corpus::Corpus, Scenario) {
    let p: Persona = serde_json::from_str(include_str!(
        "../../../fixtures/memory-bench/development/persona.json"
    ))
    .unwrap();
    let s: Scenario = serde_json::from_str(include_str!(
        "../../../fixtures/memory-bench/aging-scenario.json"
    ))
    .unwrap();
    (corpus::generate(p).unwrap(), s)
}
#[test]
fn separate_aging_fixture_keeps_base_events_and_schedules_every_calendar_day() {
    let (base, s) = fixture();
    let old = serde_json::to_vec(&base.events).unwrap();
    let c = aging::generate(base.clone(), &s, "base".into(), "scenario".into()).unwrap();
    files::validate(&c).unwrap();
    assert_eq!(c.events.len(), 2856);
    assert_eq!(c.questions.len(), 42);
    assert_eq!(c.day_end_records().unwrap().len(), 120);
    assert_eq!(c.day_end_records().unwrap().len() / 7, 17);
    let preserved = c
        .events
        .iter()
        .filter(|e| !e.record_id.starts_with("age-"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(serde_json::to_vec(&preserved).unwrap(), old);
    assert_eq!(c.persona.id, base.persona.id);
    assert!(matches!(c.profile, Profile::Aging { .. }));
    assert_eq!(corpus::counts(&c).values().sum::<usize>(), 42);
    for day in 31..=120 {
        assert_eq!(
            c.events
                .iter()
                .filter(|e| e.session_id == format!("age-day-{day:03}")
                    && e.kind == EventKind::Utterance)
                .count(),
            2
        );
    }
    for e in &c.events {
        assert!(!e.record_id.contains("low") && !e.record_id.contains("high"));
        assert!(e.cues.topic.is_none() && e.cues.objects.is_empty());
    }
    for q in &c.questions {
        assert_eq!(q.query.max_context_bytes, 4096);
        assert!(q.human_audit);
        let boundary = c
            .events
            .iter()
            .find(|e| e.record_id == q.after_record)
            .unwrap();
        assert!(q.query.at_unix_ms > boundary.at_unix_ms);
        if q.category == "aging_gist" {
            let ended = c
                .events
                .iter()
                .find(|e| e.record_id == format!("{}-end", q.id.split("-d").next().unwrap()))
                .unwrap();
            let age = q.query.at_unix_ms - ended.at_unix_ms;
            assert_eq!(age > 90 * 86_400_000, q.id.ends_with("d120"));
        }
    }
    let repeat = aging::generate(base, &s, "base".into(), "scenario".into()).unwrap();
    assert_eq!(
        hash(&serde_json::to_vec(&c).unwrap()),
        hash(&serde_json::to_vec(&repeat).unwrap())
    );
}
#[test]
fn changed_aging_threshold_schedule_or_duplicate_case_is_rejected() {
    let (base, mut s) = fixture();
    s.end_day = 90;
    assert!(aging::generate(base.clone(), &s, "b".into(), "s".into()).is_err());
    s.end_day = 120;
    s.episodes[1].day = s.episodes[0].day;
    assert!(aging::generate(base, &s, "b".into(), "s".into()).is_err());
}
