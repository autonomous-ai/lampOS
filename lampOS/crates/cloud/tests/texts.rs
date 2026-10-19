//! A text from the Autonomous app through `lamp-cloud`'s real bus wiring
//! (#1673, `docs/messages.md`): `chat.send` is acked with a run and put on
//! `chat/in`, and what a voice sends back on `chat/out` comes out as V1's
//! `chat.event` data. A voice of the test's own stands in for the lamp's.
use lamp_bus::Bus;
use lamp_cloud::answer::{Lamp, answer};
use lamp_cloud::lamp::{Places, Real, Ring, Texts, Wifi};
use lamp_msgs::{ChatIn, ChatOut, ChatPart};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A lamp on its own bus under `/tmp` (a socket's path must be short),
/// texts taken or not.
fn lamp(name: &str, texting: bool) -> (Real, PathBuf) {
    let root = PathBuf::from(format!("/tmp/lct-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bus_root = root.join("bus");
    std::fs::create_dir_all(&bus_root).unwrap();
    let lamp = Real {
        places: Places {
            actions: root.join("call.sock"),
            proc: root.join("proc"),
            sys: root.join("sys"),
            etc: root.join("etc"),
        },
        bus: Some(Bus::new(&bus_root).unwrap()),
        device_id: "d-42".into(),
        build: String::new(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::on(texting),
    };
    (lamp, bus_root)
}

/// The `chat.event`s' data the lamp gives within a second, until `enough`.
fn events_until(lamp: &mut Real, enough: usize) -> Vec<Value> {
    let until = Instant::now() + Duration::from_secs(1);
    let mut events = Vec::new();
    while events.len() < enough && Instant::now() < until {
        events.extend(lamp.chat_events(lamp_cloud::chat::now_ms()));
        std::thread::sleep(Duration::from_millis(5));
    }
    events
}

#[test]
fn a_text_goes_on_the_bus_and_its_answer_comes_back_as_v1s_chat_events() {
    let (mut lamp, bus_root) = lamp("answer", true);
    let voice = Bus::new(&bus_root).unwrap();
    let mut heard = voice
        .subscribe_typed(ChatIn::TOPIC, "voice", ChatIn::TYPE)
        .unwrap();
    let mut says = voice
        .publisher_declared(ChatOut::TOPIC, &ChatOut::declaration(), "voice")
        .unwrap();

    let ack = answer(
        &mut lamp,
        "chat.send",
        &json!({"message": "What time is it?", "session_id": "s1"}),
    )
    .unwrap()
    .unwrap();
    let run = ack["run_id"].as_str().unwrap().to_owned();
    assert_eq!(ack["session_id"], "s1");

    let message = heard
        .recv(Duration::from_secs(1))
        .unwrap()
        .expect("the text on chat/in");
    // Meaningful as long as its run waits, and passed over after: a text
    // the app has given up on is never answered (the reviewer on #1713).
    assert_eq!(
        message.header.expires_ms,
        Some(lamp_msgs::CHAT_ANSWER_WITHIN.as_millis() as u64)
    );
    let text: ChatIn = message.parse().unwrap();
    assert_eq!(text.run_id, run);
    assert_eq!(text.text, "What time is it?");
    assert!(!text.speak);

    for (part, words) in [
        (ChatPart::Delta, "It's "),
        (ChatPart::Delta, "six."),
        (ChatPart::Final, "It's six."),
    ] {
        says.publish_json(
            lamp_ipc::monotonic_us(),
            &ChatOut {
                run_id: Some(run.clone()),
                session_id: Some("s1".into()),
                part,
                text: words.into(),
            },
        )
        .unwrap();
    }
    let events = events_until(&mut lamp, 2);
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0]["event"]["type"], "assistant_delta");
    assert_eq!(events[0]["event"]["summary"], "It's six.");
    assert_eq!(events[1]["event"]["type"], "chat_response");
    assert_eq!(events[1]["event"]["state"], "final");
    assert_eq!(events[1]["event"]["detail"]["message"], "It's six.");
    assert_eq!(events[1]["run_id"], run.as_str());
    assert_eq!(events[1]["session_id"], "s1");
}

#[test]
fn with_nothing_listening_a_text_is_refused_at_once_and_off_it_is_not_here() {
    let (mut lamp, _) = lamp("nobody", true);
    assert_eq!(
        answer(&mut lamp, "chat.send", &json!({"message": "hi"})),
        Err(lamp_cloud::chat::NOBODY.to_owned())
    );
    let (mut lamp, _) = lamp_off();
    assert!(!lamp.texting());
    assert_eq!(
        answer(&mut lamp, "chat.send", &json!({"message": "hi"})),
        Err(lamp_cloud::wire::not_here("chat.send"))
    );
}

fn lamp_off() -> (Real, PathBuf) {
    lamp("off", false)
}

#[test]
fn task_news_comes_out_with_no_text_asked_first() {
    let (mut lamp, bus_root) = lamp("news", true);
    // The lamp listens from its first look.
    assert!(lamp.chat_events(lamp_cloud::chat::now_ms()).is_empty());
    let mut says = Bus::new(&bus_root)
        .unwrap()
        .publisher_declared(ChatOut::TOPIC, &ChatOut::declaration(), "voice")
        .unwrap();
    says.publish_json(
        lamp_ipc::monotonic_us(),
        &ChatOut {
            run_id: None,
            session_id: None,
            part: ChatPart::TaskNews,
            text: "Claude Code finished the report.".into(),
        },
    )
    .unwrap();
    let events = events_until(&mut lamp, 1);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["event"]["type"], "task_news");
    assert!(events[0].get("run_id").is_none());
}
