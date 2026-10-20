use lamp_msgs::{Log, LogLevel, declaration_of, first_topics, logging_topics};

#[test]
fn logging_reexports_the_canonical_type_without_changing_the_original_catalogue() {
    let log: lamp_bus::logging::Log = Log {
        part: "body".into(),
        level: LogLevel::Info,
        code: "ready".into(),
        message: "checks complete".into(),
        attempt: 1,
        created_unix_ms: 123,
    };
    assert!(log.check().is_ok());
    assert_eq!(first_topics().len(), 17);
    assert!(!first_topics().iter().any(|(topic, _)| *topic == Log::TOPIC));
    assert_eq!(logging_topics(), vec![(Log::TOPIC, Log::declaration())]);
    assert_eq!(declaration_of(Log::TOPIC), Some(Log::declaration()));
}
