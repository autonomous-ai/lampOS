//! `LAMP_OPENS_FURTHER` (#5194) on the engine: a question in a quiet room
//! is told its answer may end with one short opening; a command, a stop and
//! a busy room are asked exactly as without the switch, and so is every line
//! with the switch off.
use super::*;

/// `heard` asked as one spoken turn, answered "Sure.", with the switch `on`,
/// the room judged `busy`, and `after_stop` its first line after a hand's
/// stop: the messages round 1 was asked with.
async fn asked(on: bool, busy: bool, after_stop: bool, heard: &str) -> Vec<Message> {
    let _one = crate::provider_worker::tests::one_thread();
    *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
    *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new([vec![text("Sure."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.opening_further(on);
    if busy {
        engine = engine.judging_busy(Arc::new(|_| true));
    }
    engine.submit(ProviderInput::Start {
        request: 1,
        session: 1,
        privacy_generation: 1,
        continues: false,
        continues_cut_off: false,
        over_the_lamp: false,
        after_stop,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
        soft_lead_us: 0,
    });
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    turn(&mut engine).await;
    let asked = script.asked.lock().unwrap();
    asked[0].clone()
}

#[tokio::test]
async fn a_question_in_a_quiet_room_is_told_it_may_open_further() {
    let messages = asked(true, false, false, "What should I cook tonight").await;
    assert_eq!(
        messages.last(),
        Some(&Message::user("What should I cook tonight"))
    );
    assert_eq!(
        messages[messages.len() - 2],
        Message::system(crate::opens_further::NOTE)
    );
}

/// The issue's test: a command, a stop, a busy room and a close ("Hey Lamp,
/// okay, thanks") get no follow-up, asked byte for byte as with the switch off.
#[tokio::test]
async fn a_command_a_stop_and_a_busy_room_are_asked_as_without_the_switch() {
    for (busy, after_stop, heard) in [
        (false, false, "Set a timer for ten minutes"),
        (false, false, "Lamp, lights off"),
        (false, true, "What now"),
        (true, false, "What time is it"),
        (false, false, "Hey Lamp, okay, thanks"),
    ] {
        let on = asked(true, busy, after_stop, heard).await;
        let off = asked(false, busy, after_stop, heard).await;
        assert_eq!(on, off, "{heard}");
        assert!(
            !on.contains(&Message::system(crate::opens_further::NOTE)),
            "{heard}"
        );
    }
}

/// Without the switch, the question is asked with no note.
#[tokio::test]
async fn without_the_switch_nothing_is_told() {
    let messages = asked(false, false, false, "What should I cook tonight").await;
    assert!(!messages.contains(&Message::system(crate::opens_further::NOTE)));
}
