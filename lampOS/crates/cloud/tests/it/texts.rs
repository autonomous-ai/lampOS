//! A text from the Autonomous app through `lamp-cloud`'s real bus wiring
//! (#1673, `docs/messages.md`): `chat.send` is acked with a run and put on
//! `chat/in`, and what a voice sends back on `chat/out` comes out as V1's
//! `chat.event` data. A voice of the test's own stands in for the lamp's.
use lamp_bus::Bus;
use lamp_cloud::answer::{Lamp, answer};
use lamp_cloud::lamp::{Places, Real, Ring, Texts, Wifi};
use lamp_msgs::{ChatIn, ChatOut, ChatPart, MessageOut};
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
            run: root.join("run"),
        },
        bus: Some(Bus::new(&bus_root).unwrap()),
        device_id: "d-42".into(),
        build: String::new(),
        wifi: Wifi::default(),
        ring: Ring::default(),
        texts: Texts::on(texting),
        reports: None,
        skills: None,
        changes: None,
        photos: None,
        faces: None,
        schedule: None,
        update_page: false,
        update_checks: Default::default(),
        restart: None,
        guard: None,
        make_skill: None,
        shutdown: None,
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

/// The skills' lines heard on `messages/out` within a second, given.
fn lines_until(lamp: &mut Real, enough: usize) -> Vec<lamp_cloud::unasked::Piece> {
    let until = Instant::now() + Duration::from_secs(1);
    let mut due = Vec::new();
    while due.len() < enough && Instant::now() < until {
        due.extend(lamp.unasked(lamp_cloud::chat::now_ms(), true).due);
        std::thread::sleep(Duration::from_millis(5));
    }
    due
}

/// #2562: guard mode's named text off the bus goes named only while the
/// broker is there to take it and the eyes' `tellable`, read as it goes,
/// still has the yes of each one named. A yes taken back while the text
/// waited on the bus, before lamp-cloud first read it, sends it unnamed even
/// connected; one that comes while the broker is away is held, and goes
/// later unnamed. The journal says each once.
#[test]
fn a_named_text_goes_named_only_while_its_yes_stands_as_it_goes() {
    let (mut lamp, bus_root) = lamp("unasked-named", true);
    let known = bus_root.with_file_name("known");
    let tellable = lamp_msgs::known::tellable_of(&known);
    std::fs::write(&tellable, "Minh\n").unwrap();
    lamp.texts = Texts::on(true).knowing(Some(known.clone()));
    assert!(
        lamp.unasked(lamp_cloud::chat::now_ms(), true)
            .due
            .is_empty()
    );
    let skills = Bus::new(&bus_root).unwrap();
    let mut texts = skills
        .publisher_declared(MessageOut::TOPIC, &MessageOut::declaration(), "skills")
        .unwrap();
    let named = "Minh came in at 18:10 and was here for about 2 minutes.";
    let unnamed =
        "Someone you've introduced me to came in at 18:10 and was here for about 2 minutes.";
    let out = MessageOut {
        from: "skill:guard-mode".into(),
        text: named.into(),
        unnamed: Some(unnamed.into()),
        named: vec![lamp_msgs::known::person_id("Minh")],
    };
    let said = |event: &Value| {
        (
            event["summary"].as_str().unwrap().to_owned(),
            event["detail"]["message"].as_str().unwrap().to_owned(),
        )
    };
    // The yes stands: named.
    texts.publish_json(lamp_ipc::monotonic_us(), &out).unwrap();
    let due = lines_until(&mut lamp, 1);
    assert_eq!(due.len(), 1, "{due:?}");
    assert_eq!(said(&due[0].data["event"]), (named.into(), named.into()));
    // Published with the yes; taken back before lamp-cloud reads it off
    // the bus; then read, connected: unnamed, summary and detail both.
    texts.publish_json(lamp_ipc::monotonic_us(), &out).unwrap();
    std::fs::write(&tellable, "Lan\n").unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    let mut heard = lamp_cloud::unasked::Heard::default();
    while heard.due.is_empty() && Instant::now() < until {
        heard = lamp.unasked(lamp_cloud::chat::now_ms(), true);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(heard.due.len(), 1, "{:?}", heard.due);
    let event = &heard.due[0].data["event"];
    assert_eq!(said(event), (unnamed.into(), unnamed.into()));
    assert!(!event.to_string().contains("Minh"), "{event}");
    assert_eq!(heard.lines, [lamp_cloud::unasked::NOT_CONFIRMED]);
    // The broker away, the yes back: held, and later sent unnamed.
    std::fs::write(&tellable, "Minh\n").unwrap();
    texts.publish_json(lamp_ipc::monotonic_us(), &out).unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    let mut lines = Vec::new();
    while lines.is_empty() && Instant::now() < until {
        let heard = lamp.unasked(lamp_cloud::chat::now_ms(), false);
        assert!(heard.due.is_empty());
        lines = heard.lines;
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(lines, [lamp_cloud::unasked::HELD_UNNAMED]);
    let heard = lamp.unasked(lamp_cloud::chat::now_ms(), true);
    assert!(heard.lines.is_empty(), "said once: {:?}", heard.lines);
    assert_eq!(heard.due.len(), 1, "{:?}", heard.due);
    assert_eq!(
        said(&heard.due[0].data["event"]),
        (unnamed.into(), unnamed.into())
    );
    // No known file for lamp-cloud: never named.
    lamp.texts = Texts::on(true);
    lamp.unasked(lamp_cloud::chat::now_ms(), true);
    texts.publish_json(lamp_ipc::monotonic_us(), &out).unwrap();
    let due = lines_until(&mut lamp, 1);
    assert_eq!(due.len(), 1, "{due:?}");
    assert_eq!(
        said(&due[0].data["event"]),
        (unnamed.into(), unnamed.into())
    );
}

/// A skill's line on `messages/out`, as `lamp-skills` publishes it, comes
/// off the bus as a `task_news` event with no run and the text whole;
/// without texts taken, none is.
#[test]
fn a_skills_line_on_messages_out_comes_off_the_bus_as_task_news() {
    for texting in [true, false] {
        let name = if texting { "unasked" } else { "unasked-off" };
        let (mut lamp, bus_root) = lamp(name, texting);
        // Listening first, as from the start.
        assert!(
            lamp.unasked(lamp_cloud::chat::now_ms(), true)
                .due
                .is_empty()
        );
        let skills = Bus::new(&bus_root).unwrap();
        let mut texts = skills
            .publisher_declared(MessageOut::TOPIC, &MessageOut::declaration(), "skills")
            .unwrap();
        let text = "Someone was at your desk at 14:31, for about 2 minutes.";
        texts
            .publish_json(
                lamp_ipc::monotonic_us(),
                &MessageOut {
                    from: "skill:guard-mode".into(),
                    text: text.into(),
                    unnamed: None,
                    named: Vec::new(),
                },
            )
            .unwrap();
        if !texting {
            // Without texts taken it never listens on `messages/out`, so
            // nothing can come: one look, not a second of them (#1806).
            assert!(
                skills
                    .subscribers(MessageOut::TOPIC)
                    .unwrap()
                    .iter()
                    .all(|listener| !listener.socket.starts_with("cloud")),
            );
            let due = lamp.unasked(lamp_cloud::chat::now_ms(), true).due;
            assert!(due.is_empty(), "{due:?}");
            continue;
        }
        let due = lines_until(&mut lamp, 1);
        assert_eq!(due.len(), 1, "{due:?}");
        assert_eq!(due[0].skill, "guard-mode");
        let data = &due[0].data;
        assert!(data.get("run_id").is_none());
        assert_eq!(data["event"]["type"], "task_news");
        assert_eq!(data["event"]["summary"], text);
        assert_eq!(data["event"]["detail"]["message"], text);
        assert!(data["event"]["id"].as_str().unwrap().starts_with("evt-"));
        assert!(data["event"]["time"].is_string());
    }
}

/// A line another publisher puts on `messages/out` (a `lamp-bus` publish
/// from a shell, a test tool) is not sent, and the journal names the
/// publisher once, never the text; `lamp-skills`' line after it is.
#[test]
fn a_line_from_another_publisher_on_messages_out_is_not_sent() {
    let (mut lamp, bus_root) = lamp("unasked-elsewhere", true);
    assert!(
        lamp.unasked(lamp_cloud::chat::now_ms(), true)
            .due
            .is_empty()
    );
    let bus = Bus::new(&bus_root).unwrap();
    let line = |text: &str| MessageOut {
        from: "skill:guard-mode".into(),
        text: text.into(),
        unnamed: None,
        named: Vec::new(),
    };
    let mut shell = bus
        .publisher_declared(
            MessageOut::TOPIC,
            &MessageOut::declaration(),
            "lamp_bus_cli",
        )
        .unwrap();
    for _ in 0..2 {
        shell
            .publish_json(lamp_ipc::monotonic_us(), &line("Someone was at your desk."))
            .unwrap();
    }
    // The skill's line after them, on the same socket: once it is due, the
    // shell's two have been read before it, not a second of quiet (#1806).
    let mut skills = bus
        .publisher_declared(MessageOut::TOPIC, &MessageOut::declaration(), "skills")
        .unwrap();
    skills
        .publish_json(lamp_ipc::monotonic_us(), &line("From the skill."))
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let mut journal = Vec::new();
    let mut due = Vec::new();
    while due.is_empty() {
        assert!(Instant::now() < until, "the skill's line never came");
        let heard = lamp.unasked(lamp_cloud::chat::now_ms(), true);
        journal.extend(heard.lines);
        due.extend(heard.due);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(journal.len(), 1, "{journal:?}");
    assert!(journal[0].contains("lamp_bus_cli"), "{journal:?}");
    assert!(!journal[0].contains("desk"), "{journal:?}");
    assert_eq!(due.len(), 1, "{due:?}");
    assert_eq!(due[0].data["event"]["detail"]["message"], "From the skill.");
}
