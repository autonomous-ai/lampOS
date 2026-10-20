//! The first topics on a real bus, in a directory of the test's own: each
//! declared as this crate says, and a message of each kind sent and read
//! back as its type.
use lamp_bus::{Bus, Kind, Publisher};
use lamp_msgs::{
    Audio, Decision, Diagnostic, JointState, KeyValue, Level, Light, Speech, declaration_of,
    first_topics,
};
use std::{fs, io, path::PathBuf, time::Duration};

const WAIT: Duration = Duration::from_secs(5);

/// A root of the test's own, removed when the test ends however it ends.
/// The name is short: a socket's whole path has to fit in 103 bytes.
struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("m{:x}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_first_topic_is_declared_on_the_bus_and_carries_its_type() {
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    // The bus takes every name and every declaration: seventeen topics,
    // each with a publisher that has not spoken yet.
    let mut publishers: Vec<Publisher> = first_topics()
        .iter()
        .map(|(topic, declaration)| bus.publisher_declared(topic, declaration, "part").unwrap())
        .collect();
    assert_eq!(publishers.len(), 17);
    for (topic, declaration) in first_topics() {
        assert_eq!(
            bus.declaration(topic).unwrap(),
            Some(declaration),
            "{topic}"
        );
        assert_eq!(bus.publishers(topic).unwrap().len(), 1, "{topic}");
    }
    let listed: Vec<String> = bus
        .topics()
        .unwrap()
        .into_iter()
        .map(|topic| topic.name)
        .collect();
    let named: Vec<&str> = first_topics().iter().map(|(topic, _)| *topic).collect();
    assert_eq!(listed, named);
    let mut publisher = |topic: &str| {
        let at = publishers
            .iter()
            .position(|publisher| publisher.topic() == topic)
            .unwrap();
        publishers.swap_remove(at)
    };

    // A latched value, as JSON.
    let mut follow = bus
        .subscribe_typed(JointState::TOPIC, "t", JointState::TYPE)
        .unwrap();
    let joints = JointState {
        name: vec!["yaw".into(), "pitch".into()],
        position: vec![0.125, -0.5],
        velocity: vec![],
        effort: vec![],
        commanded: vec![0.15, -0.5],
    };
    let mut body = publisher(JointState::TOPIC);
    assert_eq!(body.publish_json(1, &joints).unwrap().delivered, 1);
    let message = follow.recv(WAIT).unwrap().expect("the joints");
    assert_eq!(message.header.type_name.as_deref(), Some(JointState::TYPE));
    assert_eq!(message.header.kind, Kind::Latest);
    assert_eq!(message.parse::<JointState>().unwrap(), joints);
    let kept = bus.latest(JointState::TOPIC).unwrap().expect("kept");
    assert_eq!(kept.parse::<JointState>().unwrap(), joints);

    // A block of sound: the header as a line, then the samples untouched.
    let mut ear = bus.subscribe_typed("mic2/audio", "t", Audio::TYPE).unwrap();
    let audio = Audio {
        sample_rate: 16_000,
        channels: 1,
        sample_format: Audio::S16LE.into(),
        sequence: 0,
    };
    let samples: Vec<u8> = (0..640u32).map(|index| (index * 7) as u8).collect();
    let mut mic = publisher("mic2/audio");
    let block = audio.block(&samples).unwrap();
    let sent = mic.publish_bytes(1, Audio::ENCODING, &block).unwrap();
    assert_eq!(sent.delivered, 1);
    let message = ear.recv(WAIT).unwrap().expect("the block");
    assert_eq!(message.header.enc, "json+pcm");
    assert_eq!(message.header.kind, Kind::Event);
    let (header, heard) = Audio::split(&message.payload).unwrap();
    assert_eq!((header, heard), (audio, &samples[..]));
    assert!(bus.latest("mic2/audio").unwrap().is_none());

    // The same type on two topics: what was heard and what was said.
    for topic in Speech::TOPICS {
        let mut listener = bus.subscribe_typed(topic, "t", Speech::TYPE).unwrap();
        let words = Speech {
            text: format!("on {topic}"),
            finished: true,
            locale: None,
            request: Some(3),
        };
        assert_eq!(
            publisher(topic).publish_json(1, &words).unwrap().delivered,
            1
        );
        let message = listener.recv(WAIT).unwrap().expect("the words");
        assert_eq!(message.parse::<Speech>().unwrap(), words);
    }

    // Every part's health on one topic.
    let mut watch = bus
        .subscribe_typed(Diagnostic::TOPIC, "t", Diagnostic::TYPE)
        .unwrap();
    let report = Diagnostic {
        level: Level::Ok,
        name: "body".into(),
        message: String::new(),
        values: vec![KeyValue {
            key: "joints".into(),
            value: "5".into(),
        }],
    };
    assert_eq!(
        publisher(Diagnostic::TOPIC)
            .publish_json(1, &report)
            .unwrap()
            .delivered,
        1
    );
    let message = watch.recv(WAIT).unwrap().expect("the report");
    assert_eq!(message.parse::<Diagnostic>().unwrap(), report);

    // What the policy decided: an event, with its answers by name.
    let mut watch = bus
        .subscribe_typed(Decision::TOPIC, "t", Decision::TYPE)
        .unwrap();
    let decided = Decision {
        about: "attention".into(),
        choice: "answer".into(),
        probability: 0.9078,
        answers: [("calls_lamp".to_string(), 0.07)].into(),
        by: "typesafe/jev-1.13-20260917".into(),
        took_ms: 655,
        shadow: true,
        request: None,
        task: None,
        arrival: None,
        ask: None,
    };
    assert_eq!(
        publisher(Decision::TOPIC)
            .publish_json(1, &decided)
            .unwrap()
            .delivered,
        1
    );
    let message = watch.recv(WAIT).unwrap().expect("the decision");
    assert_eq!(message.header.kind, Kind::Event);
    assert_eq!(message.parse::<Decision>().unwrap(), decided);
    assert!(bus.latest(Decision::TOPIC).unwrap().is_none());

    // A subscriber that expects another type of a first topic is refused,
    // and so is a publisher that would declare it as something else.
    let [command, state] = Light::TOPICS;
    let error = bus
        .subscribe_typed(command, "t", JointState::TYPE)
        .err()
        .expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(
        error
            .to_string()
            .contains("declared to carry lamp_msgs/Light"),
        "{error}"
    );
    let error = bus
        .publisher_declared(state, &JointState::declaration(), "body")
        .err()
        .expect("refused");
    assert!(
        error
            .to_string()
            .contains("already declared as lamp_msgs/Light (latest)"),
        "{error}"
    );
    // What a light's own check refuses, the bus carries all the same: it
    // compares names, and the check is the subscriber's to ask for.
    let mut ring = bus.subscribe_typed(command, "t", Light::TYPE).unwrap();
    let asked = Light::new(true, "warm", 3).unwrap();
    assert_eq!(
        publisher(command)
            .publish_json(1, &asked)
            .unwrap()
            .delivered,
        1
    );
    let light: Light = ring
        .recv(WAIT)
        .unwrap()
        .expect("the light")
        .parse()
        .unwrap();
    assert_eq!(light, asked);
    assert_eq!(light.check(), Ok(()));
    assert_eq!(declaration_of(command), Some(Light::declaration()));
}
