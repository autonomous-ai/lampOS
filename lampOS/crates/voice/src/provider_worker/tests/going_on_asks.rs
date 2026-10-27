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
    /// A `stay_quiet` in shadow only, so long after the first output (none
    /// before it).
    Shadow(Duration),
    Never,
}

/// One line heard as `text`, in a busy room with a conversation going on
/// and nobody turned to the lamp, which the service answers: the Policy's
/// decision about it as `decided` says, with the switch (`asks`) or
/// without. Whether its answer's sound came out, and when, from the first
/// of it sent by the service.
async fn going_on_line(asks: bool, decided: Decided, text: &str) -> (bool, Duration) {
    let (played, first, _) = going_on_line_with(asks, false, decided, text).await;
    (played, first)
}

/// [`going_on_line`] with `LAMP_REMARKS` too (`remarks`), and when its
/// turn's end came out, from the same moment.
async fn going_on_line_with(
    asks: bool,
    remarks: bool,
    decided: Decided,
    text: &str,
) -> (bool, Duration, Duration) {
    let _room = super::spoken_to::OwnRoom::fresh();
    let mut desk = super::policy_holds::Desk::new(if asks { "goa" } else { "gof" });
    let mut options = desk.options(false);
    options.attention = Some(busy_going_on);
    options.remarks = remarks;
    options.going_on_asks = asks.then(|| desk.bus());
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard(text)).await;
    match decided {
        Decided::Before(choice, follows) => {
            desk.decide_with(Some(request), choice, json!({"follows_lamp": follows}))
        }
        Decided::Shadow(after) if after.is_zero() => desk.decide_in_shadow(request, "stay_quiet"),
        _ => {}
    }
    let sent = crate::scenario_clock::instant();
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut late = match decided {
        Decided::After(choice, follows, after) => Some((Some(choice), follows, after)),
        Decided::Shadow(after) if !after.is_zero() => Some((None, 0.0, after)),
        _ => None,
    };
    let mut first_sound = None;
    let ended_at;
    loop {
        let since = crate::scenario_clock::instant() - sent;
        assert!(
            since < Duration::from_secs(5),
            "{text}: its turn never ended"
        );
        if let Some((choice, follows, after)) = late
            && since >= after
        {
            match choice {
                Some(choice) => {
                    desk.decide_with(Some(request), choice, json!({"follows_lamp": follows}))
                }
                None => desk.decide_in_shadow(request, "stay_quiet"),
            }
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
            ended_at = since;
            break;
        }
        rig.tick(Duration::from_millis(2)).await;
    }
    rig.stop().await;
    (
        first_sound.is_some(),
        first_sound.unwrap_or_default(),
        ended_at,
    )
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
    // A `look_up` is the lamp's in a conversation going on with it (the
    // first builder on #1522, 19:04).
    assert!(
        going_on_line(true, Decided::Before("look_up", 0.3), CAR)
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
async fn reply_after(lamp_said: &str, reply: &str, decided: bool) -> (bool, Duration) {
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
    if decided {
        desk.decide_with(Some(request), "stay_quiet", json!({"follows_lamp": 0.4}));
    }
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
    const ASKED: &str = "I can ask Claude Code to write it. Shall I start it?";
    let (played, after) = reply_after(ASKED, "Yes, go ahead.", true).await;
    assert!(played && after < Duration::from_millis(100), "{after:?}");
    // With no decision at all it doesn't wait for one either: the worker's
    // own check, not the verdict's (the reviewer on #1538, 16:48).
    let (played, after) = reply_after(ASKED, "Yes, go ahead.", false).await;
    assert!(played && after < Duration::from_millis(100), "{after:?}");
    assert!(
        !reply_after("Here is a poem about the sea.", "Yes, go ahead.", true)
            .await
            .0
    );
}

/// The reviewer on #1538 (16:21): a Policy in shadow (no `--act
/// attention`) decides every line, and nothing it decides is acted on. Its
/// decision ends the wait at once, and the line is played as without the
/// switch, though the shadow said `stay_quiet`: before the line's answer,
/// at once; 300 ms after it, then, not after the 2 s.
#[tokio::test(start_paused = true)]
async fn with_the_switch_a_shadow_decision_ends_the_wait_and_holds_nothing() {
    let _clock = logical_clock();
    const TRAIN: &str = "And how long would that take by train?";
    let (played, after) = going_on_line(true, Decided::Shadow(Duration::ZERO), TRAIN).await;
    assert!(played && after < Duration::from_millis(100), "{after:?}");
    let late = Duration::from_millis(300);
    let (played, after) = going_on_line(true, Decided::Shadow(late), TRAIN).await;
    assert!(
        played && after >= late && after < late + Duration::from_millis(100),
        "{after:?}"
    );
}

/// The reviewer on #1538 (16:21): a remark to the room, with
/// `LAMP_REMARKS` as well, is held as a remark without waiting for the
/// Policy, which has nothing to add to it. With no decision its turn ends
/// at once, not after the wait. Without `LAMP_REMARKS` the same line waits,
/// and is played when no decision comes.
#[tokio::test(start_paused = true)]
async fn with_the_switch_a_remark_held_as_one_does_not_wait_for_the_policy() {
    let _clock = logical_clock();
    const UMBRELLA: &str = "I should bring an umbrella then.";
    let (played, _, ended) = going_on_line_with(true, true, Decided::Never, UMBRELLA).await;
    assert!(!played && ended < Duration::from_millis(100), "{ended:?}");
    let (played, first, _) = going_on_line_with(true, false, Decided::Never, UMBRELLA).await;
    assert!(
        played && first >= crate::policy_holds::WAIT - Duration::from_millis(100),
        "{first:?}"
    );
}

/// A `look_up` plays in a conversation going on with the lamp (the first
/// builder on #1522, 19:04, replacing #1538's 16:51 on this): its words and
/// its call. A `stay_quiet` under 0.6 holds both. The call is the line's
/// first output, as for "And turn yourself up a little.", and the gate for
/// actions is off, as in the product units.
#[tokio::test(start_paused = true)]
async fn with_the_switch_a_look_up_plays_and_a_stay_quiet_holds_its_call_too() {
    let _clock = logical_clock();
    for (choice, runs) in [("look_up", true), ("stay_quiet", false)] {
        let _room = super::spoken_to::OwnRoom::fresh();
        let mut desk = super::policy_holds::Desk::new(if runs { "gol" } else { "gos" });
        let mut options = desk.options(false);
        options.attention = Some(busy_going_on);
        options.going_on_asks = Some(desk.bus());
        let mut rig = LiveRig::start_with(quick_policy(), options);
        let mut dial = rig.ready().await;
        let request = rig.speak(&mut dial, false, true).await;
        dial.service
            .send(heard("And turn yourself up a little."))
            .await;
        desk.decide_with(Some(request), choice, json!({"follows_lamp": 0.4}));
        dial.service
            .send(json!({"toolCall":{"functionCalls":[{"id":"v1","name":"volume_up","args":{}}]}}))
            .await;
        let told = rig.service_message(&mut dial).await;
        let response = &told["toolResponse"]["functionResponses"][0]["response"];
        assert_eq!(desk.asked.try_recv().is_ok(), runs, "{choice}: {response}");
        // What the model says after it, watched for half a second with the
        // worker's lease kept fresh: in this rig a turn that made a call is
        // not completed (`policy_holds`' tests).
        dial.service.send(provider_audio(9, 240)).await;
        let mut played = false;
        let until = crate::scenario_clock::instant() + Duration::from_millis(500);
        while crate::scenario_clock::instant() < until {
            rig.publish();
            while let Some(wire) = rig.take() {
                played |= matches!(wire, Wire::Output(ProviderOutput::Audio { request: of, .. }) if of == request);
            }
            rig.tick(Duration::from_millis(2)).await;
        }
        assert_eq!(played, runs, "{choice}: its words");
        rig.stop().await;
    }
}

/// The first builder on #1522 (19:04): on lamp-4ace at 18:4x, Daniel's
/// "Yes, give me five minutes.", to Samantha, followed the lamp by Jev's
/// `follows_lamp` and set a five-minute timer that rang mid-day. With the
/// switch, in a busy room, a reply that follows the lamp sets nothing on
/// the schedule unless its words name a timer, a reminder or the lamp; the
/// gate for actions on, as on lamp-4ace.
#[tokio::test(start_paused = true)]
async fn with_the_switch_talk_in_a_busy_room_sets_no_timer_unless_it_names_one() {
    let _clock = logical_clock();
    for (lamp_said, line, sets) in [
        (None, "Yes, give me five minutes.", false),
        (None, "And set a timer for five minutes.", true),
        // The reviewer on #1594: an answer to the lamp's own question is the
        // person's, so it sets the timer they asked for.
        (Some("For how long should I set it?"), "Ten minutes.", true),
        (
            Some("Once upon a time there was a clever fox."),
            "Yes, give me five minutes.",
            false,
        ),
    ] {
        let _room = super::spoken_to::OwnRoom::fresh();
        let mut desk = super::policy_holds::Desk::new(if sets { "gtn" } else { "gtt" });
        let mut options = desk.options(true);
        options.attention = Some(busy_going_on);
        options.going_on_asks = Some(desk.bus());
        let mut rig = LiveRig::start_with(quick_policy(), options);
        let mut dial = rig.ready().await;
        // The lamp answered a line just before, so the turn follows it.
        let first = rig.speak(&mut dial, false, true).await;
        desk.decide_with(Some(first), "answer", json!({}));
        dial.service.send(provider_audio(9, 480)).await;
        if let Some(said) = lamp_said {
            dial.service
                .send(json!({"serverContent":{"outputTranscription":{"text":said}}}))
                .await;
        }
        dial.service.send(idle_complete()).await;
        let until = crate::scenario_clock::instant() + Duration::from_secs(3);
        let mut ended = false;
        while !ended {
            assert!(
                crate::scenario_clock::instant() < until,
                "{line}: the first never ended"
            );
            rig.publish();
            while let Some(wire) = rig.take() {
                ended |= matches!(wire, Wire::Output(ProviderOutput::TurnComplete { request, .. }) if request == first);
            }
            rig.tick(Duration::from_millis(2)).await;
        }
        rig.owner
            .complete_turn(real_time(), rig.snapshot.owner().unwrap())
            .unwrap();
        rig.publish();
        let request = rig.speak(&mut dial, false, true).await;
        dial.service.send(heard(line)).await;
        desk.decide_with(
            Some(request),
            "stay_quiet",
            json!({"calls_lamp": 0.05, "follows_lamp": 0.72}),
        );
        dial.service
            .send(json!({"toolCall":{"functionCalls":[
                {"id":"t1","name":"set_timer","args":{"minutes":5}}
            ]}}))
            .await;
        let told = rig.service_message(&mut dial).await;
        let response = told["toolResponse"]["functionResponses"][0]["response"].clone();
        assert_eq!(desk.asked.try_recv().is_ok(), sets, "{line}: {response}");
        assert_eq!(
            response == json!({"done": false, "next": crate::policy_holds::HELD}),
            !sets,
            "{line}: {response}"
        );
        rig.stop().await;
    }
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

/// `--cut-in-told` comes right after `--going-on-asks`, as the coordinator
/// writes them (#828), on both routes.
#[test]
fn cut_in_told_is_read_after_going_on_asks() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::going_on_asks::FLAG, crate::cut_in_told::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.going_on_asks && files.cut_in_told);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.cut_in_told);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.cut_in_told);
    let wrong: Vec<String> = [crate::cut_in_told::FLAG, crate::going_on_asks::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// #829: `--name-note-heard-as` comes right after the name note's flags, on
/// both routes' parsers, as the coordinator passes them.
#[test]
fn the_heard_as_line_is_read_after_the_name_note() {
    let dir = std::path::Path::new("/run/test");
    let flags =
        |list: &[&str]| -> Vec<String> { list.iter().map(|flag| (*flag).to_owned()).collect() };
    let with = flags(&[crate::called::FLAG, crate::called::HEARD_AS_FLAG]);
    let (_, _, files, _) = parse_flags(dir, &with).unwrap();
    assert!(files.name_note && files.name_note_heard_as);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &with).unwrap();
    assert!(files.name_note_heard_as);
    let busy = flags(&[
        crate::called::FLAG,
        crate::called::BUSY_FLAG,
        crate::called::HEARD_AS_FLAG,
    ]);
    let (_, _, files, _) = parse_flags(dir, &busy).unwrap();
    assert!(files.name_note && files.name_note_busy_face && files.name_note_heard_as);
    let (_, _, files, _) = parse_flags(dir, &with[..1]).unwrap();
    assert!(!files.name_note_heard_as);
    let wrong = flags(&[crate::called::HEARD_AS_FLAG, crate::called::FLAG]);
    assert!(parse_flags(dir, &wrong).is_err());
}

/// #828: `--misheard-hum` comes right after `--cut-in-told`, on both routes'
/// parsers, as the coordinator passes them.
#[test]
fn misheard_hum_is_read_after_cut_in_told() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::cut_in_told::FLAG, crate::misheard_hum::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.cut_in_told && files.misheard_hum);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.misheard_hum);
    let (_, _, files, _) = parse_flags(dir, &flags[1..]).unwrap();
    assert!(files.misheard_hum && !files.cut_in_told);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.misheard_hum);
    let wrong: Vec<String> = [crate::misheard_hum::FLAG, crate::cut_in_told::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// #1673: `--messages` comes after `--misheard-hum` and `--holding-answer`,
/// on both routes' parsers, as the coordinator passes them.
#[test]
fn messages_is_read_after_misheard_hum_and_the_holding_answer() {
    let dir = std::path::Path::new("/run/test");
    let all: Vec<String> = [
        crate::misheard_hum::FLAG,
        crate::holding_answer::FLAG,
        crate::messages::FLAG,
    ]
    .map(String::from)
    .into();
    let (_, _, files, _) = parse_flags(dir, &all).unwrap();
    assert!(files.misheard_hum && files.holding_answer && files.messages);
    let held_first: Vec<String> = [crate::messages::FLAG, crate::holding_answer::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &held_first).is_err());
    let flags: Vec<String> = [crate::misheard_hum::FLAG, crate::messages::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.misheard_hum && files.messages);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.messages);
    let (_, _, files, _) = parse_flags(dir, &flags[1..]).unwrap();
    assert!(files.messages && !files.misheard_hum);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.messages);
    let wrong: Vec<String> = [crate::messages::FLAG, crate::misheard_hum::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// #2136: `--others-answer` comes right after `--messages`, as the
/// coordinator writes them, on both routes' parsers (the worker's
/// environment is cleared, so this is how the switch reaches the rule).
#[test]
fn others_answer_is_read_after_messages() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::messages::FLAG, crate::others_answer::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.messages && files.others_answer);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.others_answer);
    let (_, _, files, _) = parse_flags(dir, &flags[1..]).unwrap();
    assert!(files.others_answer && !files.messages);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.others_answer);
    let wrong: Vec<String> = [crate::others_answer::FLAG, crate::messages::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// `--said-as-heard` comes after `--messages`, on both routes' parsers, as
/// the coordinator passes them; only the second route acts on it.
#[test]
fn said_as_heard_is_read_after_messages() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::messages::FLAG, crate::said_as_heard::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.messages && files.said_as_heard);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.said_as_heard);
    let (_, _, files, _) = parse_flags(dir, &flags[1..]).unwrap();
    assert!(files.said_as_heard && !files.messages);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.said_as_heard);
    let wrong: Vec<String> = [crate::said_as_heard::FLAG, crate::messages::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// #2191: `--going-on-same-one` comes right after `--others-answer`, as the
/// coordinator writes them, on both routes' parsers.
#[test]
fn going_on_same_one_is_read_after_others_answer() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::others_answer::FLAG, crate::going_on_same_one::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.others_answer && files.going_on_same_one);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.going_on_same_one);
    let (_, _, files, _) = parse_flags(dir, &flags[1..]).unwrap();
    assert!(files.going_on_same_one && !files.others_answer);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.going_on_same_one);
    let wrong: Vec<String> = [crate::going_on_same_one::FLAG, crate::others_answer::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// #3882: `--turned-away` comes right after `--going-on-same-one`, as the
/// coordinator writes them, on both routes' parsers.
#[test]
fn turned_away_is_read_after_going_on_same_one() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::going_on_same_one::FLAG, crate::turned_away::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.going_on_same_one && files.turned_away);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.turned_away);
    let (_, _, files, _) = parse_flags(dir, &flags[1..]).unwrap();
    assert!(files.turned_away && !files.going_on_same_one);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.turned_away);
    let wrong: Vec<String> = [crate::turned_away::FLAG, crate::going_on_same_one::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// `--name-or-follow-up` comes right after `--live-scoped-session`, and
/// `--follow-up-for-ms` with its value right after it, as the coordinator
/// writes them, on both routes' parsers.
#[test]
fn name_or_follow_up_is_read_after_the_scoped_session() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::scoped_session::FLAG, crate::name_or_follow_up::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.scoped_session && files.name_or_follow_up);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.name_or_follow_up);
    let (_, _, files, _) = parse_flags(dir, &flags[1..]).unwrap();
    assert!(files.name_or_follow_up && !files.scoped_session);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.name_or_follow_up);
    let wrong: Vec<String> = [crate::name_or_follow_up::FLAG, crate::scoped_session::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
    // `LAMP_FOLLOW_UP_FOR_MS`'s flag and value right after it, on both
    // routes; 8000, the window as unset, since the window is the process's.
    let window = format!("{}", crate::name_or_follow_up::FOLLOW_UP_FOR.as_millis());
    let flags: Vec<String> = [
        crate::scoped_session::FLAG,
        crate::name_or_follow_up::FLAG,
        crate::name_or_follow_up::FOR_FLAG,
        &window,
    ]
    .map(String::from)
    .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.scoped_session && files.name_or_follow_up);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.name_or_follow_up);
    let wrong: Vec<String> = [
        crate::name_or_follow_up::FOR_FLAG,
        &window,
        crate::name_or_follow_up::FLAG,
    ]
    .map(String::from)
    .into();
    assert!(parse_flags(dir, &wrong).is_err());
}
