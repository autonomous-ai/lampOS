use super::*;

fn read(engine: &str, gateway: Option<Gateway>) -> Read {
    Read {
        engine: Some(engine.to_owned()),
        gateway,
    }
}

const READY: Option<Gateway> = Some(Gateway::Ready);
const SWITCHING: Option<Gateway> = Some(Gateway::Switching);

/// A conversation open (number 1) that has read OpenClaw as the default.
fn talking_on_openclaw() -> SwitchTold {
    let mut told = SwitchTold::default();
    told.conversation(Some(1));
    told.take(1, &read("openclaw", READY), 0);
    told
}

#[test]
fn a_switch_in_a_conversation_is_said_then_its_ready_line() {
    let mut told = talking_on_openclaw();
    assert_eq!(told.owed(), None, "the first read is no switch");
    told.take(1, &read("hermes", SWITCHING), 1_000_000);
    assert_eq!(told.owed(), Some((Agent::Hermes, Moment::Switching)));
    told.said(Moment::Switching);
    assert_eq!(told.owed(), None, "not ready yet");
    told.take(1, &read("hermes", READY), 2_000_000);
    assert_eq!(told.owed(), Some((Agent::Hermes, Moment::Ready)));
    told.said(Moment::Ready);
    assert_eq!(told.owed(), None);
}

#[test]
fn with_no_conversation_nothing_is_read_or_owed() {
    // An app switch while the room is quiet: no conversation is open, so a
    // read (one in flight as the conversation ended) is passed over.
    let mut told = SwitchTold::default();
    told.take(0, &read("openclaw", READY), 0);
    told.take(0, &read("hermes", SWITCHING), 1_000_000);
    told.take(0, &read("hermes", READY), 5_000_000);
    assert_eq!(told.owed(), None);
    // A conversation that opens after the switch starts from what is read
    // then: the switch made in the quiet room is never said.
    told.conversation(Some(1));
    told.take(1, &read("hermes", SWITCHING), 6_000_000);
    told.take(1, &read("hermes", READY), 9_000_000);
    assert_eq!(told.owed(), None);
}

#[test]
fn a_conversation_that_ends_forgets_the_switch_and_its_lines() {
    let mut told = talking_on_openclaw();
    told.take(1, &read("hermes", SWITCHING), 1_000_000);
    assert!(told.owed().is_some());
    told.conversation(None);
    assert_eq!(told.owed(), None, "never into an empty room");
    // A read of the ended conversation, landing late, is passed over too.
    told.take(1, &read("hermes", READY), 9_000_000);
    assert_eq!(told.owed(), None);
    // The next conversation is its own: the switch it did not see is not
    // said, nor its ready line.
    told.conversation(Some(2));
    told.take(2, &read("hermes", READY), 10_000_000);
    assert_eq!(told.owed(), None);
}

#[test]
fn each_line_is_said_once_whatever_is_read_again() {
    let mut told = talking_on_openclaw();
    for at in 1..=5 {
        told.take(1, &read("hermes", SWITCHING), at * 1_000_000);
    }
    assert_eq!(told.owed(), Some((Agent::Hermes, Moment::Switching)));
    told.said(Moment::Switching);
    for at in 6..=10 {
        told.take(1, &read("hermes", SWITCHING), at * 1_000_000);
        assert_eq!(told.owed(), None, "switching said once");
    }
    for at in 11..=15 {
        told.take(1, &read("hermes", READY), at * 1_000_000);
    }
    assert_eq!(told.owed(), Some((Agent::Hermes, Moment::Ready)));
    told.said(Moment::Ready);
    for at in 16..=20 {
        told.take(1, &read("hermes", READY), at * 1_000_000);
        assert_eq!(told.owed(), None, "ready said once");
    }
    // Switching back is a new switch, with its own two lines.
    told.take(1, &read("openclaw", SWITCHING), 21_000_000);
    assert_eq!(told.owed(), Some((Agent::OpenClaw, Moment::Switching)));
}

#[test]
fn no_ready_line_without_its_switching_line_said() {
    let mut told = talking_on_openclaw();
    told.take(1, &read("hermes", SWITCHING), 1_000_000);
    told.take(1, &read("hermes", READY), 2_000_000);
    // Ready read, but "switching" not said yet: that comes first, never
    // "ready" on its own.
    assert_eq!(told.owed(), Some((Agent::Hermes, Moment::Switching)));
    // A ready line with no switch at all is never owed.
    let mut quiet = talking_on_openclaw();
    quiet.take(1, &read("openclaw", SWITCHING), 1_000_000);
    quiet.take(1, &read("openclaw", READY), 9_000_000);
    assert_eq!(quiet.owed(), None);
    quiet.said(Moment::Ready);
    assert_eq!(quiet.owed(), None);
}

#[test]
fn a_ready_read_at_the_change_is_taken_only_once_settled() {
    // The engine changed, and the gateway still reads ready: the old
    // gateway's word, before lamp-tasks looked again.
    let mut told = talking_on_openclaw();
    told.take(1, &read("hermes", READY), 1_000_000);
    told.said(Moment::Switching);
    assert_eq!(told.owed(), None, "a ready read at the change is not taken");
    told.take(1, &read("hermes", READY), 1_000_000 + SETTLE_US - 1);
    assert_eq!(told.owed(), None);
    // Past the settle time (a gateway with no direct session reads ready
    // at once and for good), it is.
    told.take(1, &read("hermes", READY), 1_000_000 + SETTLE_US);
    assert_eq!(told.owed(), Some((Agent::Hermes, Moment::Ready)));
}

#[test]
fn with_no_gateway_given_no_ready_line_is_said() {
    // A lamp-tasks that gives no `gateway` on `targets`: the switch is still
    // said, its ready line never.
    let mut told = talking_on_openclaw();
    told.take(1, &read("hermes", None), 1_000_000);
    assert_eq!(told.owed(), Some((Agent::Hermes, Moment::Switching)));
    told.said(Moment::Switching);
    told.take(1, &read("hermes", None), 60_000_000);
    assert_eq!(told.owed(), None);
}

#[test]
fn a_switch_to_another_engine_is_not_said() {
    let mut told = talking_on_openclaw();
    told.take(1, &read("claude", SWITCHING), 1_000_000);
    assert_eq!(told.owed(), None);
    // ...and a switch on from it to Hermes is.
    told.take(1, &read("hermes", SWITCHING), 2_000_000);
    assert_eq!(told.owed(), Some((Agent::Hermes, Moment::Switching)));
}

#[test]
fn a_read_names_the_lamps_engine_and_its_gateway() {
    let answer = json!({"ok":true,"gateway":"switching","targets":[
        {"alias":"office","engine":"claude"},
        {"alias":"lamp","engine":"hermes","engines":["hermes","openclaw"]},
    ]});
    assert_eq!(
        Read::of(&answer),
        Some(Read {
            engine: Some("hermes".into()),
            gateway: SWITCHING
        })
    );
    let older = json!({"ok":true,"targets":[{"alias":"lamp","engine":"openclaw"}]});
    assert_eq!(
        Read::of(&older),
        Some(Read {
            engine: Some("openclaw".into()),
            gateway: None
        })
    );
    let starting = json!({"ok":true,"lamp":"starting","targets":[]});
    assert_eq!(Read::of(&starting).unwrap().engine, None);
    assert_eq!(Read::of(&json!({"ok":false,"error":"busy"})), None);
}

#[test]
fn every_language_has_both_lines_for_both_agents_said_as_written() {
    let languages = [
        None,
        Some(Language::Vietnamese),
        Some(Language::Japanese),
        Some(Language::Chinese),
        Some(Language::Korean),
        Some(Language::French),
    ];
    for agent in [Agent::OpenClaw, Agent::Hermes] {
        let name = match agent {
            Agent::OpenClaw => "OpenClaw",
            Agent::Hermes => "Hermes",
        };
        for language in languages {
            let switching = line(language, agent, Moment::Switching);
            let ready = line(language, agent, Moment::Ready);
            assert_ne!(switching, ready, "{language:?}");
            for words in [switching, ready] {
                // The agent named as it is written, in every language.
                assert!(words.contains(name), "{language:?}: {words}");
                let note = crate::onboarding::said_as_is(words)
                    .unwrap_or_else(|| panic!("{language:?}: {words} can't be said as written"));
                assert_eq!(crate::onboarding::own_words(&note), Some(words));
            }
        }
    }
    assert_eq!(
        line(None, Agent::Hermes, Moment::Switching),
        "Switching to Hermes."
    );
    assert_eq!(
        line(None, Agent::OpenClaw, Moment::Ready),
        "OpenClaw is ready."
    );
    // Each language its own words, never English's.
    for language in &languages[1..] {
        assert_ne!(
            line(*language, Agent::Hermes, Moment::Ready),
            "Hermes is ready."
        );
    }
}

/// A task the switch ended is told as the lamp's exact line in its
/// language, said as written, never a prompt for the chat model.
#[test]
fn a_task_the_switch_ended_is_told_the_lamps_exact_line_in_its_language() {
    let english = ended_note(None).unwrap();
    assert_eq!(
        crate::onboarding::own_words(&english),
        Some("That stopped because I switched to another agent. Please ask me again.")
    );
    assert_eq!(
        crate::onboarding::own_words(&ended_note(Some(Language::Vietnamese)).unwrap()),
        Some("Việc đó đã dừng vì mình chuyển sang agent khác. Bạn hỏi lại mình nhé.")
    );
    let mut seen = vec![english];
    for language in [
        Language::Vietnamese,
        Language::Japanese,
        Language::Chinese,
        Language::Korean,
        Language::French,
    ] {
        let note = ended_note(Some(language))
            .unwrap_or_else(|| panic!("{language:?}: can't be said as written"));
        assert!(!seen.contains(&note), "{language:?}: {note}");
        seen.push(note);
    }
}

#[test]
fn the_handle_says_a_conversations_line_once_and_none_after_it_ends() {
    // No lamp-tasks at this socket: nothing is read; the handle's own
    // conversation numbering is what is tested.
    let mut handle = AgentSwitchTold::start(Path::new("/nonexistent/lamp-tasks/request.sock"));
    handle.conversation(true, 0);
    let first = handle.shared.conversation.load(Ordering::Relaxed);
    assert_ne!(first, 0);
    handle.conversation(true, 100);
    assert_eq!(handle.shared.conversation.load(Ordering::Relaxed), first);
    let take = |handle: &AgentSwitchTold, read: &Read, at: u64| {
        handle.shared.told.lock().unwrap().take(first, read, at);
    };
    take(&handle, &read("openclaw", READY), 0);
    take(&handle, &read("hermes", SWITCHING), 1_000_000);
    let (moment, note) = handle.owed().expect("owed in the conversation");
    assert_eq!(moment, Moment::Switching);
    assert_eq!(
        crate::onboarding::own_words(&note),
        Some(line(
            crate::language::at_start(),
            Agent::Hermes,
            Moment::Switching
        ))
    );
    handle.said(moment);
    assert!(handle.owed().is_none());
    // A moment with no sign (a reply ended, its exchange not yet in the
    // thread) does not end it...
    take(&handle, &read("openclaw", SWITCHING), 2_000_000);
    handle.conversation(false, 2_000_000);
    handle.conversation(false, 2_000_000 + ENDS_AFTER_US - 1);
    assert_eq!(handle.shared.conversation.load(Ordering::Relaxed), first);
    assert!(handle.owed().is_some(), "still owed in the conversation");
    handle.conversation(true, 2_000_000 + ENDS_AFTER_US);
    handle.conversation(false, 3_000_000 + ENDS_AFTER_US);
    assert_eq!(handle.shared.conversation.load(Ordering::Relaxed), first);
    // ...its end does: nothing owed, and its reads are passed over.
    handle.conversation(false, 3_000_000 + 2 * ENDS_AFTER_US);
    assert_eq!(handle.shared.conversation.load(Ordering::Relaxed), 0);
    take(&handle, &read("hermes", READY), 9_000_000);
    assert!(handle.owed().is_none());
    // A new conversation has a new number.
    handle.conversation(true, 9_000_000 + 3 * ENDS_AFTER_US);
    assert_ne!(handle.shared.conversation.load(Ordering::Relaxed), first);
}

#[test]
fn in_a_conversation_is_a_turn_open_or_the_thread_going_on() {
    assert!(in_conversation(true, || false));
    assert!(in_conversation(false, || true));
    assert!(!in_conversation(false, || false));
}
