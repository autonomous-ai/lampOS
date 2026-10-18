//! In a busy room, a line let out only as a conversation going on waits for
//! the Policy's decision and is played only on `answer` or on a `stay_quiet`
//! said back to the lamp (#1522, `LAMP_GOING_ON_ASKS=on`), through the
//! worker's own loop against the scripted service and a bus of the test's
//! own ([`super::policy_holds::Desk`]).
use super::*;

/// A busy room, and a conversation with the lamp going on.
fn busy_going_on() -> (crate::room_told::Room, bool) {
    (crate::room_told::Room::Busy, true)
}

fn heard(text: &str) -> serde_json::Value {
    json!({"serverContent":{"inputTranscription":{"text":text}}})
}

/// When the Policy's decision about a line comes, against the line's first
/// output from the service: its choice and its `follows_lamp`.
#[derive(Clone, Copy)]
enum Decided {
    Before(&'static str, f32),
    After(&'static str, f32, Duration),
    Never,
}

/// One line heard as `text`, in a busy room with a conversation going on
/// and nobody turned to the lamp, which the service answers: the Policy's
/// decision about it as `decided` says, with the switch (`asks`) or
/// without. Whether its answer's sound came out, and when, from the first
/// of it sent by the service.
async fn going_on_line(asks: bool, decided: Decided, text: &str) -> (bool, Duration) {
    let _room = super::spoken_to::OwnRoom::fresh();
    let mut desk = super::policy_holds::Desk::new(if asks { "goa" } else { "gof" });
    let mut options = desk.options(false);
    options.attention = Some(busy_going_on);
    options.going_on_asks = asks.then(|| desk.bus());
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard(text)).await;
    if let Decided::Before(choice, follows) = decided {
        desk.decide_with(Some(request), choice, json!({"follows_lamp": follows}));
    }
    let sent = crate::scenario_clock::instant();
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut late = match decided {
        Decided::After(choice, follows, after) => Some((choice, follows, after)),
        _ => None,
    };
    let mut first_sound = None;
    loop {
        let since = crate::scenario_clock::instant() - sent;
        assert!(
            since < Duration::from_secs(5),
            "{text}: its turn never ended"
        );
        if let Some((choice, follows, after)) = late
            && since >= after
        {
            desk.decide_with(Some(request), choice, json!({"follows_lamp": follows}));
            late = None;
        }
        rig.publish();
        let mut ended = false;
        while let Some(wire) = rig.take() {
            match wire {
                Wire::Output(ProviderOutput::Audio { request: of, .. }) if of == request => {
                    first_sound.get_or_insert(since);
                }
                Wire::Output(ProviderOutput::TurnComplete { request: of, .. }) if of == request => {
                    ended = true;
                }
                _ => {}
            }
        }
        if ended {
            break;
        }
        rig.tick(Duration::from_millis(2)).await;
    }
    rig.stop().await;
    (first_sound.is_some(), first_sound.unwrap_or_default())
}

/// The battery's scene 3 on lamp-4ace at 14:44 (#1522), with the Policy's
/// own decisions on its lines then: the car line (`stay_quiet`) and scene
/// 4's opener (`stay_quiet`) are held, and "And what about tomorrow?"
/// (`stay_quiet`, `follows_lamp` 0.66) is played, at once, its decision
/// being there before its answer. So is a `look_up` held. Without the
/// switch the car line is played, as it was.
#[tokio::test(start_paused = true)]
async fn with_the_switch_a_line_going_on_in_a_busy_room_is_played_only_as_the_policy_says() {
    let _clock = logical_clock();
    const CAR: &str = "Okay, I will book the car for eight.";
    const OPENER: &str = "Yes, I read it this morning. The numbers look high.";
    const TOMORROW: &str = "And what about tomorrow?";
    assert!(
        !going_on_line(true, Decided::Before("stay_quiet", 0.12), CAR)
            .await
            .0
    );
    assert!(
        !going_on_line(true, Decided::Before("stay_quiet", 0.08), OPENER)
            .await
            .0
    );
    assert!(
        !going_on_line(true, Decided::Before("look_up", 0.3), CAR)
            .await
            .0
    );
    let (played, after) = going_on_line(true, Decided::Before("stay_quiet", 0.66), TOMORROW).await;
    assert!(played && after < Duration::from_millis(100), "{after:?}");
    let (played, after) = going_on_line(true, Decided::Before("answer", 0.2), TOMORROW).await;
    assert!(played && after < Duration::from_millis(100), "{after:?}");
    let (played, after) = going_on_line(false, Decided::Before("stay_quiet", 0.12), CAR).await;
    assert!(played && after < Duration::from_millis(100), "{after:?}");
}

/// A decision that comes after the line's first output is waited for: its
/// sound comes out only once `answer` comes, and none of it on a
/// `stay_quiet`. With none in time the line is played after the wait, as
/// without the switch: a Policy that is down does not make the lamp deaf.
#[tokio::test(start_paused = true)]
async fn with_the_switch_a_line_going_on_waits_for_a_late_decision_and_is_played_with_none() {
    let _clock = logical_clock();
    const TRAIN: &str = "And how long would that take by train?";
    let late = Duration::from_millis(300);
    let (played, after) = going_on_line(true, Decided::After("answer", 0.2, late), TRAIN).await;
    assert!(
        played && after >= late && after < crate::policy_holds::WAIT,
        "{after:?}"
    );
    assert!(
        !going_on_line(true, Decided::After("stay_quiet", 0.1, late), TRAIN)
            .await
            .0
    );
    let (played, after) = going_on_line(true, Decided::Never, TRAIN).await;
    assert!(
        played && after >= crate::policy_holds::WAIT - Duration::from_millis(100),
        "{after:?}"
    );
}

/// A line that calls the lamp in its words is played as before, at once:
/// it does not wait, and a `stay_quiet` does not hold it.
#[tokio::test(start_paused = true)]
async fn with_the_switch_a_line_that_calls_the_lamp_does_not_wait() {
    let _clock = logical_clock();
    const CALL: &str = "Lamp, how long would that take by train?";
    for decided in [Decided::Never, Decided::Before("stay_quiet", 0.1)] {
        let (played, after) = going_on_line(true, decided, CALL).await;
        assert!(played && after < Duration::from_millis(100), "{after:?}");
    }
}

/// After the lamp's answer to a call, `lamp_said`, the line `reply` with the
/// Policy's `stay_quiet` at `follows_lamp` 0.4, decided before its answer:
/// whether the reply's answer was played, and how long after its first
/// output.
async fn reply_after(lamp_said: &str, reply: &str) -> (bool, Duration) {
    let _room = super::spoken_to::OwnRoom::fresh();
    let mut desk = super::policy_holds::Desk::new("gor");
    let mut options = desk.options(false);
    options.attention = Some(busy_going_on);
    options.going_on_asks = Some(desk.bus());
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let call = rig.speak(&mut dial, false, true).await;
    dial.service
        .send(heard(
            "Hey Lamp, ask Claude Code to write a poem about the sea.",
        ))
        .await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service
        .send(json!({"serverContent":{"outputTranscription":{"text":lamp_said}}}))
        .await;
    dial.service.send(idle_complete()).await;
    loop {
        if matches!(rig.output().await, ProviderOutput::TurnComplete { request, .. } if request == call)
        {
            break;
        }
    }
    rig.owner
        .complete_turn(real_time(), rig.snapshot.owner().unwrap())
        .unwrap();
    rig.publish();
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard(reply)).await;
    desk.decide_with(Some(request), "stay_quiet", json!({"follows_lamp": 0.4}));
    let sent = crate::scenario_clock::instant();
    dial.service.send(provider_audio(9, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut first_sound = None;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: of, .. } if of == request => {
                first_sound.get_or_insert(crate::scenario_clock::instant() - sent);
            }
            ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
            _ => {}
        }
    }
    rig.stop().await;
    (first_sound.is_some(), first_sound.unwrap_or_default())
}

/// Eighth on #1538: a reply to the lamp's own question is the person's
/// answer, as the remark arm has it. "Yes, go ahead." to "Shall I start
/// it?" is played at once though the Policy said `stay_quiet` and read it
/// as not following (0.4): held, the hand-over would never run. After a
/// line that asked nothing, the same reply is held.
#[tokio::test(start_paused = true)]
async fn with_the_switch_a_reply_to_the_lamps_question_does_not_wait_and_is_not_held() {
    let _clock = logical_clock();
    let (played, after) = reply_after(
        "I can ask Claude Code to write it. Shall I start it?",
        "Yes, go ahead.",
    )
    .await;
    assert!(played && after < Duration::from_millis(100), "{after:?}");
    assert!(
        !reply_after("Here is a poem about the sea.", "Yes, go ahead.")
            .await
            .0
    );
}

/// A line whose turn is cancelled while it waits for the Policy is let go
/// on the next tick, not after the wait, and judged once: none of its sound
/// comes out, and the worker takes the service's notices again, so the
/// next line, a call, is answered at once.
#[tokio::test(start_paused = true)]
async fn with_the_switch_a_line_whose_turn_ends_while_it_waits_is_let_go_at_once() {
    let _clock = logical_clock();
    let _room = super::spoken_to::OwnRoom::fresh();
    let desk = super::policy_holds::Desk::new("goc");
    let mut options = desk.options(false);
    options.attention = Some(busy_going_on);
    options.going_on_asks = Some(desk.bus());
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service
        .send(heard("And how long would that take by train?"))
        .await;
    dial.service.send(provider_audio(7, 240)).await;
    let sound_of = |rig: &mut LiveRig, of: u64| {
        let mut sound = false;
        while let Some(wire) = rig.take() {
            sound |= matches!(wire, Wire::Output(ProviderOutput::Audio { request, .. }) if request == of);
        }
        sound
    };
    // Kept: nothing of it comes out while it waits.
    let until = crate::scenario_clock::instant() + Duration::from_millis(200);
    while crate::scenario_clock::instant() < until {
        rig.publish();
        assert!(!sound_of(&mut rig, request), "played while it waited");
        rig.tick(Duration::from_millis(2)).await;
    }
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.cancel_turn(real_time(), owner).unwrap();
    rig.publish();
    let cancelled = crate::scenario_clock::instant();
    dial.service.send(idle_complete()).await;
    let next = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard("Hey Lamp, what time is it?")).await;
    dial.service.send(provider_audio(9, 240)).await;
    loop {
        let since = crate::scenario_clock::instant() - cancelled;
        assert!(
            since < Duration::from_secs(1),
            "the next call waited {since:?}"
        );
        assert!(!rig.worker.is_finished(), "the worker ended");
        rig.publish();
        let mut answered = false;
        while let Some(wire) = rig.take() {
            assert!(
                !matches!(wire, Wire::Output(ProviderOutput::Audio { request: of, .. }) if of == request),
                "the cancelled line was played"
            );
            answered |= matches!(wire, Wire::Output(ProviderOutput::Audio { request: of, .. }) if of == next);
        }
        if answered {
            break;
        }
        rig.tick(Duration::from_millis(2)).await;
    }
    rig.stop().await;
}

/// `--going-on-asks` is read where the coordinator puts it, after
/// `--remarks`, on both routes; out of its place it is refused.
#[test]
fn going_on_asks_is_read_after_remarks() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::remarks::FLAG, crate::going_on_asks::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.remarks && files.going_on_asks);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.going_on_asks);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.going_on_asks);
    let wrong: Vec<String> = [crate::going_on_asks::FLAG, crate::remarks::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}
