//! A short sound let through in a quiet room (`LAMP_SHORT_SOUNDS=quiet-room`)
//! and checked (`quiet-room-checked`, #800), through the worker's own loop
//! against the scripted service: the request's sound is ended marked as a
//! sound, the service writes down what it heard, and the model answers.
use super::*;

fn quiet() -> (crate::room_told::Room, bool) {
    (crate::room_told::Room::Quiet, false)
}

fn checked(on: bool) -> Options {
    Options {
        reporting: Reporting::Typed,
        tools: crate::tools::Settings::default(),
        attention: Some(quiet),
        name_signal: false,
        sounds_checked: on,
        ask_unclear: false,
        name_in_passing_ignored: false,
        remarks: false,
        going_on_asks: None,
        cut_in_told: false,
        misheard_hum: false,
        policy_holds: None,
        follows_run: false,
        wait_for_room: false,
        staff_note: false,
        spoken_to: false,
        voices: None,
        catalog: None,
    }
}

fn heard(text: &str) -> serde_json::Value {
    json!({"serverContent":{"inputTranscription":{"text":text}}})
}

/// A request whose sound the coordinator ended as a sound let through,
/// heard as `text`, and answered by the model: whether the answer's sound
/// came out of the worker.
async fn answered(options: Options, text: &str) -> bool {
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let played = sound_answered(&mut rig, &mut dial, text).await;
    rig.stop().await;
    played
}

/// In `rig`, a request whose sound the coordinator ended as a sound let
/// through, heard as `text`: whether its answer's sound came out.
async fn sound_answered(rig: &mut LiveRig, dial: &mut Dial, text: &str) -> bool {
    let request = rig.speak(dial, false, false).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: true,
            cut_in: false,
        })
        .unwrap();
    let ended = rig.service_message(dial).await;
    assert!(
        ended["realtimeInput"].get("activityEnd").is_some(),
        "{ended}"
    );
    dial.service.send(heard(text)).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    dial.service.send(provider_audio(9, 480)).await;
    dial.service.send(idle_complete()).await;
    let first = rig.output().await;
    let played = matches!(first, ProviderOutput::Audio { request: got, .. } if got == request);
    if !played {
        assert!(
            matches!(first, ProviderOutput::TurnComplete { request: done, .. } if done == request),
            "{first:?}"
        );
    }
    played
}

/// On lamp-52e6 on 2026-10-15 at 13:03 a room sound of 0.7 s, let through
/// in a quiet room, was heard as "¿Qué?" and answered. Checked, it is not;
/// a short word said to the lamp still is; and without the switch both are
/// answered, as before.
#[tokio::test]
async fn a_sound_heard_as_another_language_is_held_and_a_short_word_is_answered() {
    assert!(!answered(checked(true), "¿Qué?").await);
    assert!(answered(checked(true), "Yes.").await);
    assert!(answered(checked(false), "¿Qué?").await);
}

/// The worker reads `--sounds-checked` where the coordinator puts it, after
/// the first-output file and before the camera; and the second voice's
/// worker, which takes the same flags, takes it too, rather than refusing
/// it as unknown and leaving the lamp without its second voice, and checks
/// sounds with it (`second_provider`'s own test holds the answer).
#[test]
fn both_workers_take_the_flag_that_checks_sounds() {
    let dir = std::path::Path::new("/run/lamp-voice");
    let flags = |list: &[&str]| {
        list.iter()
            .map(|flag| (*flag).to_owned())
            .collect::<Vec<_>>()
    };
    let checked = flags(&[
        "--attention",
        crate::first_output::FLAG,
        "/run/lamp-voice/first-output",
        crate::not_speech::CHECKED_FLAG,
    ]);
    let (_, _, files, visual) = parse_flags(dir, &checked).unwrap();
    assert!(files.attention && files.sounds_checked && visual.is_none());
    let (_, _, files, _) = parse_flags(dir, &flags(&["--attention"])).unwrap();
    assert!(!files.sounds_checked);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &checked).unwrap();
    assert!(crate::second_provider::worker::checks_sounds(&files));
    // As on the Live route, only with the attention rule on.
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(
        dir,
        &flags(&[crate::not_speech::CHECKED_FLAG]),
    )
    .unwrap();
    assert!(files.sounds_checked && !crate::second_provider::worker::checks_sounds(&files));
}

fn talking() -> (crate::room_told::Room, bool) {
    (crate::room_told::Room::Quiet, true)
}

/// The scene on lamp-4ace on 2026-10-17 at 16:11, checked, in a
/// conversation, with the conversation as the worker itself keeps it: a
/// first request answered with `lamp_line`, then a short sound heard as
/// `text`. Whether the sound's answer came out.
async fn sound_after(lamp_line: &str, text: &str) -> bool {
    let _room = super::spoken_to::OwnRoom::fresh();
    let options = Options {
        attention: Some(talking),
        ..checked(true)
    };
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let asked = rig.speak(&mut dial, false, true).await;
    dial.service
        .send(heard("Can you get the office build checked?"))
        .await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service
        .send(json!({"serverContent":{"outputTranscription":{"text":lamp_line}}}))
        .await;
    dial.service.send(idle_complete()).await;
    loop {
        if let ProviderOutput::TurnComplete { request, .. } = rig.output().await
            && request == asked
        {
            break;
        }
    }
    rig.owner
        .complete_turn(real_time(), rig.snapshot.owner().unwrap())
        .unwrap();
    rig.publish();
    let played = sound_answered(&mut rig, &mut dial, text).await;
    rig.stop().await;
    played
}

/// "Yes. Please start it." to the lamp's "Shall I start it?" was ended
/// after "Yes." and heard as "bien": checked, its answer was held and its
/// handing over never run. Right after the lamp's own question it is the
/// person's answer; after a line that asks nothing, it is still held.
#[tokio::test]
async fn a_sound_with_a_word_right_after_the_lamps_question_is_answered() {
    assert!(sound_after("I can hand that to Codex. Shall I start it?", "bien").await);
    assert!(!sound_after("I can hand that to Codex whenever you like.", "bien").await);
}

/// In a conversation, with the switch, a turn ended as speech (a cut-in is
/// never a sound) and heard as `text`: whether its answer came out, and
/// whether the worker said it was unclear, before its `TurnComplete`.
async fn in_conversation(ask_unclear: bool, text: &str) -> (bool, Option<String>) {
    in_conversation_ended(ask_unclear, text, false).await
}

/// As [`in_conversation`], the turn ended as a cut-in over the lamp's answer
/// or not.
async fn in_conversation_ended(
    ask_unclear: bool,
    text: &str,
    cut_in: bool,
) -> (bool, Option<String>) {
    let options = Options {
        attention: Some(talking),
        ask_unclear,
        ..checked(false)
    };
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, false).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in,
        })
        .unwrap();
    let ended = rig.service_message(&mut dial).await;
    assert!(
        ended["realtimeInput"].get("activityEnd").is_some(),
        "{ended}"
    );
    dial.service.send(heard(text)).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    // Two pieces of the answer: a held request is said unclear once.
    dial.service.send(provider_audio(9, 480)).await;
    dial.service.send(provider_audio(9, 480)).await;
    dial.service.send(idle_complete()).await;
    let mut unclear = None;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: got, .. } if got == request => {
                rig.stop().await;
                return (true, unclear);
            }
            ProviderOutput::Unclear {
                request: got, why, ..
            } if got == request => {
                assert!(unclear.is_none(), "said unclear twice");
                unclear = Some(why);
            }
            ProviderOutput::TurnComplete { request: done, .. } if done == request => {
                rig.stop().await;
                return (false, unclear);
            }
            other => panic!("{other:?}"),
        }
    }
}

/// #926: on lamp-4ace on 2026-10-16 at 05:39, in a conversation, a cut-in
/// heard as Hindi was answered in Hindi, and one heard as "23" got "Four."
/// With the switch both are held and said unclear; English is answered;
/// without the switch everything is answered, as before.
#[tokio::test]
async fn in_a_conversation_a_turn_heard_in_another_script_or_as_a_bare_number_is_asked_again() {
    let (played, why) = in_conversation(true, "हिंदू के स्थान").await;
    assert!(!played);
    assert_eq!(why.as_deref(), Some(crate::not_speech::OTHER_SCRIPT));
    let (played, why) = in_conversation(true, "23").await;
    assert!(!played);
    assert_eq!(why.as_deref(), Some(crate::not_speech::ONLY_DIGITS));
    assert_eq!(
        in_conversation(true, "No no no, tell me a joke instead.").await,
        (true, None)
    );
    assert_eq!(in_conversation(false, "हिंदू के स्थान").await, (true, None));
}

/// #926, the first builder's addition: an English cut-in over a story came
/// back as "vestido". Ended as a cut-in, a fragment of one or two words with
/// no English word in it is asked again; "Stop." is answered; the same
/// fragment as an ordinary turn is left to the model.
#[tokio::test]
async fn a_cut_in_heard_as_a_fragment_is_asked_again_and_stop_is_answered() {
    let (played, why) = in_conversation_ended(true, "vestido", true).await;
    assert!(!played);
    assert_eq!(why.as_deref(), Some(crate::not_speech::CUT_IN_FRAGMENT));
    assert_eq!(
        in_conversation_ended(true, "Stop.", true).await,
        (true, None)
    );
    assert_eq!(
        in_conversation_ended(true, "vestido", false).await,
        (true, None)
    );
}

/// #828, the first builder at 05:07 on 19 October: in a conversation, a
/// turn heard as `text`, ended as a cut-in over the lamp's answer or not,
/// and then two more requests, each begun at once. What the worker said of
/// the first (`Unclear`'s why and whether the lamp hums for it), and what
/// the service was given as text beside each of the next two, up to its end.
async fn misheard_then_two(
    misheard_hum: bool,
    text: &str,
    cut_in: bool,
) -> (Option<(String, bool)>, Vec<String>, Vec<String>) {
    let options = Options {
        attention: Some(talking),
        ask_unclear: true,
        misheard_hum,
        ..checked(false)
    };
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, false).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in,
        })
        .unwrap();
    let ended = rig.service_message(&mut dial).await;
    assert!(
        ended["realtimeInput"].get("activityEnd").is_some(),
        "{ended}"
    );
    dial.service.send(heard(text)).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    dial.service.send(provider_audio(9, 480)).await;
    dial.service.send(idle_complete()).await;
    let mut unclear = None;
    loop {
        match rig.output().await {
            ProviderOutput::Unclear {
                request: got,
                why,
                cut_in,
            } if got == request => unclear = Some((why, cut_in)),
            ProviderOutput::TurnComplete { request: done, .. } if done == request => break,
            ProviderOutput::Audio { request: got, .. } if got == request => {}
            other => panic!("{other:?}"),
        }
    }
    let second = told_beside_the_next(&mut rig, &mut dial).await;
    let third = told_beside_the_next(&mut rig, &mut dial).await;
    rig.stop().await;
    (unclear, second, third)
}

/// The person speaks again: a request ended as speech, what the service is
/// given as text beside it up to its end, and then its answer, to its end.
async fn told_beside_the_next(rig: &mut LiveRig, dial: &mut Dial) -> Vec<String> {
    let next = rig.speak(dial, false, false).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request: next,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();
    let mut told = Vec::new();
    loop {
        let message = rig.service_message(dial).await;
        if let Some(text) = message["realtimeInput"]["text"].as_str() {
            told.push(text.to_owned());
        }
        if message["realtimeInput"].get("activityEnd").is_some() {
            break;
        }
    }
    dial.service.send(heard("And then what happened?")).await;
    dial.service.send(provider_audio(9, 480)).await;
    dial.service.send(idle_complete()).await;
    loop {
        if let ProviderOutput::TurnComplete { request: done, .. } = rig.output().await
            && done == next
        {
            break;
        }
    }
    told
}

fn has_note(told: &[String]) -> bool {
    told.iter()
        .any(|text| text.contains(crate::misheard_hum::NOTE))
}

/// #828, the first builder at 05:07 on 19 October: of 59 "Sorry?" turns the
/// model was asked for after a misheard cut-in, it said "Sorry?" 13 times
/// and went back to its story about 40 (the Fourth's reader). With
/// `LAMP_MISHEARD_HUM`, a misheard cut-in is said unclear for the
/// coordinator's "hm?" in place of a model turn, and the next request,
/// once, is told it wasn't caught. A line held as unclear that was no cut-in
/// is asked again as before, and the request after it told nothing; so is a
/// cut-in heard right, and everything without the switch (the reviewer on
/// #1659: once, and cut-ins only).
#[tokio::test]
async fn with_the_switch_a_misheard_cut_in_is_said_as_one_and_the_next_request_is_told() {
    let (unclear, second, third) = misheard_then_two(true, "vestido", true).await;
    assert_eq!(
        unclear,
        Some((crate::not_speech::CUT_IN_FRAGMENT.to_owned(), true))
    );
    assert!(has_note(&second), "{second:?}");
    assert!(!has_note(&third), "told twice: {third:?}");

    let (unclear, second, _) = misheard_then_two(true, "हिंदू के स्थान", false).await;
    assert_eq!(
        unclear,
        Some((crate::not_speech::OTHER_SCRIPT.to_owned(), false))
    );
    assert!(!has_note(&second), "{second:?}");

    let (unclear, second, _) = misheard_then_two(true, "Okay, okay, that is enough.", true).await;
    assert_eq!(unclear, None);
    assert!(!has_note(&second), "{second:?}");

    let (unclear, second, _) = misheard_then_two(false, "vestido", true).await;
    assert_eq!(
        unclear,
        Some((crate::not_speech::CUT_IN_FRAGMENT.to_owned(), false))
    );
    assert!(!has_note(&second), "{second:?}");
}
