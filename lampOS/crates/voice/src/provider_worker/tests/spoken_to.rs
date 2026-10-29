//! The lamp speaks when spoken to (`LAMP_SPOKEN_TO=on`, #829, step 2),
//! through the worker's own loop against the scripted service: in a room
//! not known to be busy and outside a conversation with the lamp, an
//! answer nobody asked the lamp for is held, and so is any lamp action it
//! calls for.
use super::*;

fn quiet() -> (crate::room_told::Room, bool) {
    (crate::room_told::Room::Quiet, false)
}

fn quiet_going_on() -> (crate::room_told::Room, bool) {
    (crate::room_told::Room::Quiet, true)
}

fn options(
    spoken_to: bool,
    room: fn() -> (crate::room_told::Room, bool),
    tools: crate::tools::Settings,
) -> Options {
    Options {
        reporting: Reporting::Typed,
        holding_answer: false,
        tools,
        attention: Some(room),
        name_signal: false,
        sounds_checked: false,
        name_in_passing_ignored: false,
        remarks: false,
        going_on_asks: None,
        cut_in_told: false,
        scoped_session: false,
        misheard_hum: false,
        policy_holds: None,
        follows_run: false,
        wait_for_room: false,
        staff_note: false,
        ask_unclear: false,
        spoken_to,
        voices: None,
        catalog: None,
    }
}

fn heard(text: &str) -> serde_json::Value {
    json!({"serverContent":{"inputTranscription":{"text":text}}})
}

/// The first builder's two-person scene (#829, 02:16): the opening line,
/// said by one person to another, was answered 11 times in 12. And a line
/// heard from across the office on lamp-4ace at 20:51 on 15 October.
const NOT_TO_THE_LAMP: [&str; 2] = [
    "Did you get my message about the budget?",
    "Voglio mangiare sushi.",
];
const TO_THE_LAMP: &str = "Hey Lamp, what is the weather like in Hanoi today?";

/// A request heard as `text` and answered: whether the answer's sound came
/// out of the worker.
async fn answered(options: Options, text: &str) -> bool {
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard(text)).await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut played = false;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: of, .. } if of == request => played = true,
            ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
            _ => {}
        }
    }
    rig.stop().await;
    played
}

#[tokio::test]
async fn what_nobody_said_to_the_lamp_is_held_with_the_switch_and_played_without() {
    let tools = crate::tools::Settings::default;
    for text in NOT_TO_THE_LAMP {
        assert!(
            !answered(options(true, quiet, tools()), text).await,
            "{text}"
        );
        assert!(
            answered(options(false, quiet, tools()), text).await,
            "{text}"
        );
    }
}

#[tokio::test]
async fn the_lamp_called_by_name_or_in_a_conversation_is_answered_with_the_switch() {
    let tools = crate::tools::Settings::default;
    assert!(answered(options(true, quiet, tools()), TO_THE_LAMP).await);
    // B's follow-up in the first builder's scene, "And what about
    // tomorrow?", while the lamp is in a conversation.
    assert!(
        answered(
            options(true, quiet_going_on, tools()),
            "And what about tomorrow?"
        )
        .await
    );
}

/// #1108 through the worker's own loop: the lamp's name said alone (its
/// call's wait run out: `ProviderInput::Called`, then its End), and then
/// "Check my work." as a request of its own, with the switch, in a room not
/// known to be busy and no conversation going on. After "Lamp." the call
/// is the lamp's, and the request's answer plays; after "Hey Clamp?", a
/// call whose own hearing shows nothing, it is held, as it would be with
/// no call before it. On the logical clock: the verdict rides on the 3 s
/// from the call's End, which a host stall could pass (the Seventh on
/// #1228).
#[tokio::test(start_paused = true)]
async fn a_request_after_a_bare_call_is_answered_only_when_the_call_showed_the_lamp() {
    let _clock = logical_clock();
    assert!(after_a_bare_call("Lamp.").await);
    assert!(!after_a_bare_call("Hey Clamp?").await);
}

/// A bare call heard as `call`, answered and complete, then "Check my
/// work.": whether that request's answer came out of the worker.
async fn after_a_bare_call(call: &str) -> bool {
    // Its heard lines go into the process's thread: kept out of the other
    // tests' conversation (the reviewer on #1228).
    let _room = OwnRoom::fresh();
    let mut rig = LiveRig::start_with(
        quick_policy(),
        options(true, quiet, crate::tools::Settings::default()),
    );
    let mut dial = rig.ready().await;
    let first = rig.speak(&mut dial, false, false).await;
    dial.service.send(heard(call)).await;
    rig.parent
        .data
        .send(ProviderInput::Called {
            request: first,
            privacy_generation: rig.snapshot.microphone_generation(),
        })
        .unwrap();
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request: first,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();
    // The service is told the call came alone, then its end.
    let note = rig.service_message(&mut dial).await;
    assert!(note["realtimeInput"]["text"].is_string(), "{note}");
    let end = rig.service_message(&mut dial).await;
    assert!(end["realtimeInput"].get("activityEnd").is_some(), "{end}");
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    loop {
        if let ProviderOutput::TurnComplete { request, .. } = rig.output().await
            && request == first
        {
            break;
        }
    }
    let second = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard("Check my work.")).await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut played = false;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request, .. } if request == second => played = true,
            ProviderOutput::TurnComplete { request, .. } if request == second => break,
            _ => {}
        }
    }
    rig.stop().await;
    played
}

/// A request heard as `text` whose answer calls for the lamp action
/// `action`: what the service was answered, and what the executive was
/// asked, if it was asked at all.
async fn acted(
    spoken_to: bool,
    text: &str,
    action: &str,
) -> (serde_json::Value, Option<serde_json::Value>) {
    use std::io::{BufRead, Write};
    let home = crate::process::SessionDirectory::create().unwrap();
    let listener =
        std::os::unix::net::UnixListener::bind(home.path.join(crate::tools::CALL_SOCKET)).unwrap();
    listener.set_nonblocking(true).unwrap();
    // The executive, which changes the lamp if it is asked, until the
    // service has its answer.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let executive = std::thread::spawn({
        let done = done.clone();
        move || {
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut line = String::new();
                        std::io::BufReader::new(stream.try_clone().unwrap())
                            .read_line(&mut line)
                            .unwrap();
                        stream
                            .write_all(b"{\"ok\":true,\"result\":{\"changed\":true}}\n")
                            .unwrap();
                        return Some(serde_json::from_str::<serde_json::Value>(&line).unwrap());
                    }
                    Err(_) if done.load(std::sync::atomic::Ordering::Acquire) => return None,
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(2)),
                }
            }
        }
    });
    let mut rig = LiveRig::start_with(
        quick_policy(),
        options(
            spoken_to,
            quiet,
            crate::tools::Settings {
                decline: false,
                exec: Some(home.path.clone()),
                memory: None,
                clock: false,
                ..Default::default()
            },
        ),
    );
    let mut dial = rig.ready().await;
    rig.speak(&mut dial, false, true).await;
    dial.service.send(heard(text)).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    dial.service.send(function_call("c9", action)).await;
    // A call that is run is answered only after the executive has done it.
    let answer = rig.service_message(&mut dial).await;
    done.store(true, std::sync::atomic::Ordering::Release);
    let asked = executive.join().unwrap();
    rig.stop().await;
    (answer, asked)
}

/// The first builder's first test for step 2 (#829, 02:00): on 15 October
/// at 14:49 a word overheard in the office's talk turned the lamp's volume
/// up for 11 hours. A turn nobody said to the lamp never runs a lamp
/// action; without the switch, as before, it does.
#[tokio::test]
async fn a_turn_nobody_said_to_the_lamp_never_changes_the_lamp() {
    for action in ["volume_up", "volume_mute", "light_on", "light_colour"] {
        for text in NOT_TO_THE_LAMP.into_iter().chain(["Thailand."]) {
            let (answer, asked) = acted(true, text, action).await;
            assert_eq!(
                answer,
                json!({"toolResponse":{"functionResponses":[
                    {"id":"c9","name":action,"response":{"ok":true}}
                ]}}),
                "{action} {text}"
            );
            assert_eq!(asked, None, "{action} {text}");
        }
        let (_, asked) = acted(false, NOT_TO_THE_LAMP[0], action).await;
        assert_eq!(
            asked,
            Some(json!({"op":"call","name":action,"args":{"colour":"blue"}})),
            "{action}"
        );
    }
}

/// Its second: the same turn with the lamp's name changes the lamp.
#[tokio::test]
async fn the_same_turn_with_the_lamps_name_changes_the_lamp() {
    for action in ["volume_up", "volume_mute", "light_on", "light_colour"] {
        let (answer, asked) = acted(true, "Hey Lamp, turn yourself up a bit.", action).await;
        assert_eq!(
            answer,
            json!({"toolResponse":{"functionResponses":[
                {"id":"c9","name":action,"response":{"changed":true}}
            ]}}),
            "{action}"
        );
        assert_eq!(
            asked,
            Some(json!({"op":"call","name":action,"args":{"colour":"blue"}})),
            "{action}"
        );
    }
}

/// Both workers take `--spoken-to` where the coordinator puts it, right
/// after `--name-in-passing-ignored`: the second voice's worker, which
/// takes the same flags, would otherwise refuse it and leave the lamp
/// without its second voice. That route has no attention rule, and does
/// not apply it.
#[test]
fn both_workers_take_the_spoken_to_flag_in_its_place() {
    let dir = std::path::Path::new("/run/lamp-voice");
    let flags = [
        crate::attention::FLAG,
        crate::attention::BUSY_FLAG,
        crate::attention::NEAR_FLAG,
        crate::attention::PASSING_FLAG,
        crate::attention::SPOKEN_TO_FLAG,
        crate::first_output::FLAG,
        "/run/lamp-voice/first-output",
    ]
    .map(str::to_owned);
    let (_, _, files, visual) = parse_flags(dir, &flags).unwrap();
    assert!(files.attention && files.name_in_passing_ignored && files.spoken_to);
    assert!(visual.is_none());
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.spoken_to);
    let (_, _, files, _) = parse_flags(dir, &flags[..3]).unwrap();
    assert!(!files.spoken_to);
    assert!(!files.spoken_to_by_day);
    // `LAMP_SPOKEN_TO=day-and-night`: the day's own bars right after it.
    let by_day = [
        crate::attention::FLAG,
        crate::attention::SPOKEN_TO_FLAG,
        crate::attention::BY_DAY_FLAG,
        crate::first_output::FLAG,
        "/run/lamp-voice/first-output",
    ]
    .map(str::to_owned);
    let (_, _, files, _) = parse_flags(dir, &by_day).unwrap();
    assert!(files.spoken_to && files.spoken_to_by_day);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &by_day).unwrap();
    assert!(files.spoken_to && files.spoken_to_by_day);
}

/// Puts the process-wide mark of the lamp's own turn back when dropped,
/// even when a test fails: the worker marks its own turn for the whole
/// process, and each rig numbers its requests from one.
struct OwnMark(u64);

impl OwnMark {
    fn kept() -> Self {
        Self(crate::attention::own_request().load(std::sync::atomic::Ordering::Acquire))
    }
}

impl Drop for OwnMark {
    fn drop(&mut self) {
        crate::attention::own_request().store(self.0, std::sync::atomic::Ordering::Release);
    }
}

/// How the lamp's own turn before the room's line ended.
#[derive(Clone, Copy, Debug)]
enum OwnTurn {
    /// Its sound went out.
    Spoke,
    /// The service completed it with nothing to say.
    Silent,
    /// Its owner was cancelled before any sound.
    Cancelled,
}

/// The reviewer on #918 at 05:11, through the worker's own loop: the lamp's
/// own written turn, ended as `own`, then the two-person scene's unnamed
/// opening line in a room not known to be busy. Whether that line's answer
/// is played: only after news the lamp spoke is it a reply.
async fn after_the_lamps_own_turn(own: OwnTurn) -> bool {
    let _mark = OwnMark::kept();
    let mut rig = LiveRig::start_with(
        quick_policy(),
        options(true, quiet, crate::tools::Settings::default()),
    );
    let mut dial = rig.ready().await;
    let owner = rig
        .owner
        .admit(real_time(), lamp_turn_owner::AdmittedInput::NewTurn)
        .unwrap();
    let news = owner.turn();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::Written {
            request: news,
            session: dial.session.get(),
            privacy_generation: rig.snapshot.microphone_generation(),
            text: "Tell them the task they gave you is done.".to_owned(),
            look: false,
            search: false,
            quoted: false,
        })
        .unwrap();
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Started { request, .. } if request == news
    ));
    let written = rig.service_message(&mut dial).await;
    assert!(written.get("clientContent").is_some(), "{written}");
    match own {
        OwnTurn::Spoke => {
            dial.service.send(provider_audio(5, 240)).await;
            dial.service.send(idle_complete()).await;
        }
        OwnTurn::Silent => dial.service.send(idle_complete()).await,
        OwnTurn::Cancelled => {
            rig.owner.cancel_turn(real_time(), owner).unwrap();
            rig.publish();
            dial.service.send(idle_complete()).await;
        }
    }
    // A cancelled turn's own output is never delivered; the others end.
    if !matches!(own, OwnTurn::Cancelled) {
        loop {
            match rig.output().await {
                ProviderOutput::TurnComplete { request, .. } if request == news => break,
                _ => {}
            }
        }
    }
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard(NOT_TO_THE_LAMP[0])).await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut played = false;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: of, .. } if of == request => played = true,
            ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
            _ => {}
        }
    }
    rig.stop().await;
    played
}

#[tokio::test]
async fn news_the_lamp_spoke_is_replied_to() {
    assert!(after_the_lamps_own_turn(OwnTurn::Spoke).await);
}

#[tokio::test]
async fn an_own_turn_completed_silent_is_no_news() {
    assert!(!after_the_lamps_own_turn(OwnTurn::Silent).await);
}

#[tokio::test]
async fn an_own_turn_cancelled_before_it_spoke_is_no_news() {
    assert!(!after_the_lamps_own_turn(OwnTurn::Cancelled).await);
}

/// The reviewer on #918 at 06:17, through the worker's own loop: the
/// lamp's own written turn says two pieces of words, which use up the output
/// credit, and then its sound, which waits for credit. Proven taken from the
/// service by a later call for the time, which is answered at once. Then
/// either its owner is cancelled before the credit comes back, and that
/// sound is never handed on; or the credit comes back, and it is. Whether
/// the room's unnamed line after it is played: only after news the lamp
/// actually handed on is it a reply.
async fn after_queued_news(cancelled: bool) -> bool {
    let _mark = OwnMark::kept();
    let mut rig = LiveRig::start_with(
        quick_policy(),
        options(
            true,
            quiet,
            crate::tools::Settings {
                clock: true,
                ..Default::default()
            },
        ),
    );
    let mut dial = rig.ready().await;
    let owner = rig
        .owner
        .admit(real_time(), lamp_turn_owner::AdmittedInput::NewTurn)
        .unwrap();
    let news = owner.turn();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::Written {
            request: news,
            session: dial.session.get(),
            privacy_generation: rig.snapshot.microphone_generation(),
            text: "Tell them the task they gave you is done.".to_owned(),
            look: false,
            search: false,
            quoted: false,
        })
        .unwrap();
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Started { request, .. } if request == news
    ));
    rig.service_message(&mut dial).await;
    // Two pieces of words, read and their credit kept back.
    for text in ["Your task", " is done."] {
        dial.service
            .send(json!({"serverContent":{"outputTranscription":{"text":text,"finished":false}}}))
            .await;
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut taken = 0;
    while taken < 2 {
        rig.publish();
        if let Some(wire) = rig.parent.data.receive::<Wire>().unwrap() {
            assert!(
                matches!(wire, Wire::Output(ProviderOutput::Transcript { .. })),
                "{wire:?}"
            );
            rig.output_flow.received().unwrap();
            taken += 1;
        }
        assert!(Instant::now() < deadline, "the words did not come");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    // The sound waits for credit; the call after it shows it was taken.
    dial.service.send(provider_audio(5, 240)).await;
    dial.service
        .send(function_call("t1", crate::tools::THE_TIME))
        .await;
    let answered = rig.service_message(&mut dial).await;
    assert!(answered.get("toolResponse").is_some(), "{answered}");
    assert!(
        rig.parent.data.receive::<Wire>().unwrap().is_none(),
        "no credit, no output"
    );
    if cancelled {
        rig.owner.cancel_turn(real_time(), owner).unwrap();
        for _ in 0..25 {
            rig.publish();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
    rig.output_flow
        .replenish(0, &mut rig.parent.control)
        .unwrap();
    // The call's turn, then the answer after it.
    dial.service.send(idle_complete()).await;
    dial.service.send(idle_complete()).await;
    if !cancelled {
        loop {
            match rig.output().await {
                ProviderOutput::TurnComplete { request, .. } if request == news => break,
                _ => {}
            }
        }
    } else {
        for _ in 0..25 {
            rig.publish();
            assert!(
                !matches!(
                    rig.take(),
                    Some(Wire::Output(ProviderOutput::Audio { request, .. })) if request == news
                ),
                "a cancelled turn's sound was handed on"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard(NOT_TO_THE_LAMP[0])).await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut played = false;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: of, .. } if of == request => played = true,
            ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
            _ => {}
        }
    }
    rig.stop().await;
    played
}

#[tokio::test]
async fn news_whose_sound_waited_for_credit_and_then_went_is_replied_to() {
    assert!(after_queued_news(false).await);
}

#[tokio::test]
async fn news_whose_sound_a_cancellation_dropped_before_it_went_is_no_news() {
    assert!(!after_queued_news(true).await);
}

/// Gives the test a room and a conversation of its own, as the worker keeps
/// them (the room for its thread, the conversation for the process), and
/// puts the ones before back when dropped, even when the test fails.
pub(super) struct OwnRoom {
    room: Option<crate::room_told::RoomTold>,
    thread: Option<crate::thread::Thread>,
    unanswered: u32,
    /// Held while the test runs: the thread is one for the whole process
    /// (`crate::thread::THREAD`), so two of these tests at once wrote over
    /// each other's conversation, and one failed in half the parallel runs
    /// of the six (the eighth, 01:0x on #705). Every rig holds it too
    /// (#1422).
    _one: super::OneThread,
}

impl OwnRoom {
    pub(super) fn fresh() -> Self {
        let one = super::one_thread();
        Self {
            _one: one,
            room: Some(std::mem::take(
                &mut *crate::room_told::room()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            )),
            thread: Some(std::mem::take(
                &mut *crate::thread::THREAD
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            )),
            unanswered: unanswered_requests().load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl Drop for OwnRoom {
    fn drop(&mut self) {
        unanswered_requests().store(self.unanswered, std::sync::atomic::Ordering::Relaxed);
        if let Some(room) = self.room.take() {
            *crate::room_told::room()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = room;
            crate::room_told::room().clear_poison();
        }
        if let Some(thread) = self.thread.take() {
            *crate::thread::THREAD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = thread;
            crate::thread::THREAD.clear_poison();
        }
    }
}

/// The first builder's scene of 06:07 on lamp-4ace (#918, 06:15), through
/// the worker's own loop, with the room and the conversation as the worker
/// itself keeps them (`attention_now`): "Hey Lamp, how much is one hundred
/// dollars in euros?" answered, then the two people's lines, of which the
/// service answers none, then Daniel's "Did you get my message about the
/// budget?" to Samantha. Whether its answer was played.
async fn after_lines_left_alone(lines: &[&str]) -> bool {
    let _mark = OwnMark::kept();
    let _room = OwnRoom::fresh();
    let (mut rig, mut dial) = in_conversation(options(
        true,
        attention_now,
        crate::tools::Settings::default(),
    ))
    .await;
    for line in lines {
        left_alone(&mut rig, &mut dial, line).await;
    }
    let played = played(&mut rig, &mut dial, NOT_TO_THE_LAMP[0]).await;
    rig.stop().await;
    played
}

/// A rig with `options` in which "Hey Lamp, how much is one hundred dollars
/// in euros?" was answered, its turn ended.
async fn in_conversation(options: Options) -> (LiveRig, Dial) {
    let mut rig = LiveRig::start_configured(
        quick_policy(),
        options,
        BARRIER,
        false,
        Continuation::Off,
        lamp_gemini::MAX_WIRE_BYTES * 2,
        |config| {
            config
                .timeouts(lamp_gemini::Timeouts {
                    unanswered: Some(Duration::from_millis(100)),
                    ..Default::default()
                })
                .unwrap()
        },
    );
    let mut dial = rig.ready().await;
    let euros = rig.speak(&mut dial, false, true).await;
    dial.service
        .send(heard("Hey Lamp, how much is one hundred dollars in euros?"))
        .await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service
        .send(json!({"serverContent":{"outputTranscription":{"text":"One hundred dollars is about eighty-nine euros right now."}}}))
        .await;
    dial.service.send(idle_complete()).await;
    let mut answered = false;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request, .. } if request == euros => answered = true,
            ProviderOutput::TurnComplete { request, .. } if request == euros => break,
            _ => {}
        }
    }
    assert!(answered, "the call was not answered");
    complete(&mut rig);
    (rig, dial)
}

/// The turn ended, as the coordinator ends it.
fn complete(rig: &mut LiveRig) {
    rig.owner
        .complete_turn(real_time(), rig.snapshot.owner().unwrap())
        .unwrap();
    rig.publish();
}

/// `line` heard, and nothing comes back: the worker closes it as
/// unanswered.
async fn left_alone(rig: &mut LiveRig, dial: &mut Dial, line: &str) {
    let request = rig.speak(dial, false, true).await;
    dial.service.send(heard(line)).await;
    ended_unplayed(rig, request, line).await;
    complete(rig);
}

/// Waits for `request`'s end, none of its sound played.
async fn ended_unplayed(rig: &mut LiveRig, request: u64, line: &str) {
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: of, .. } if of == request => {
                panic!("{line} was answered")
            }
            ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
            _ => {}
        }
    }
}

/// `line` heard, and the service answers it in words: whether they were
/// played.
async fn played(rig: &mut LiveRig, dial: &mut Dial, line: &str) -> bool {
    let request = rig.speak(dial, false, true).await;
    dial.service.send(heard(line)).await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut played = false;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: of, .. } if of == request => played = true,
            ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
            _ => {}
        }
    }
    played
}

#[tokio::test]
async fn with_the_switch_two_lines_the_lamp_left_alone_end_its_conversation() {
    const BETWEEN: [&str; 2] = [
        "Anyway, if we drop the trip we save almost half.",
        "True, but the client wants to meet in person.",
    ];
    // Both lines left alone: the people went on talking to each other.
    assert!(!after_lines_left_alone(&BETWEEN).await);
    // One: the conversation with the lamp is still going on, as before.
    assert!(after_lines_left_alone(&BETWEEN[..1]).await);
}

/// `LAMP_NAME_OR_FOLLOW_UP` on Live, through the worker's own loop with the
/// room and the conversation as it keeps them (`attention_now`): after "Hey
/// Lamp, how much is one hundred dollars in euros?" was answered in a busy
/// room, a follow-up begun 5 s after the lamp's words ended is answered as
/// the conversation going on, and so is one begun then that goes on 7 s, its
/// answer 12 s after the words (a reviewer's P2 on #2967: the window runs
/// to the line's start, not its verdict); one begun 12 s after, without the
/// name, is held, inside the thread's 45 s; with the name it is answered.
/// Without the switch the 12 s one is answered too. A follow-up begun while
/// the answer still plays is answered, however long it goes on (a run on
/// lamp-52e6 on 22 Oct lost three begun as the answers played). In a room
/// not known to be busy the windows are as without the switch: one begun
/// 15 s after the words is answered (the owner at 18:0x). On the logical
/// clock, so that the long lines pass in a moment.
#[tokio::test(start_paused = true)]
async fn with_name_or_follow_up_a_follow_up_goes_on_only_eight_seconds() {
    // The worker and its backdated answer share an epoch with 15 s of history.
    let clock =
        crate::scenario_clock::Scope::enter_at(Duration::from_secs(60), 600_000_000).unwrap();
    clock.set_pump(|_| Ok(())).unwrap();
    let follow_up = "And what about tomorrow?";
    for (on, busy, ago_s, long_s, line, answered) in [
        (false, true, 12, 0, follow_up, true),
        (
            true,
            true,
            12,
            0,
            "Hey Lamp, and what about tomorrow?",
            true,
        ),
        (true, true, 12, 0, follow_up, false),
        (true, true, 5, 0, follow_up, true),
        (true, true, 5, 7, follow_up, true),
        (true, true, -2, 0, follow_up, true),
        (true, true, -2, 16, follow_up, true),
        (true, false, 15, 0, follow_up, true),
        (true, false, 12, 0, follow_up, true),
    ] {
        assert_eq!(
            after_the_lamps_words(on, busy, ago_s, long_s, line).await,
            answered,
            "{on}, busy {busy}, begun {ago_s} s after the words, {long_s} s long: {line}"
        );
    }
}

/// `LAMP_NAME_OR_FOLLOW_UP`'s known limit on Live, as built: the end of the
/// lamp's words is when the words it handed on would have played, as the
/// second route counts its own, and a barge-in that cuts them is not
/// counted back. An answer of 8 s handed on at once and cut 2 s in by a
/// barge-in: a line begun 13 s after it was handed on is 5 s after that
/// projected end, a follow-up, where 11 s after the cut it would not be.
/// The worker's loop can't be driven to it: the rig's Start never says how
/// much of a cut reply was heard (`heard_of_cut`), so this is the
/// helpers' own arithmetic. The thread's 45 s still bound it.
#[test]
fn a_barge_in_leaves_lives_follow_up_window_counting_from_the_words_projected_end() {
    use std::sync::atomic::Ordering::Relaxed;
    let before = said_until_us().swap(0, Relaxed);
    let handed_on = 1_000_000_000;
    said_through(8 * lamp_gemini::OUTPUT_RATE as usize, handed_on);
    let until = said_until_us().load(Relaxed);
    assert_eq!(until, handed_on + 8_000_000);
    let cut = handed_on + 2_000_000;
    let line = handed_on + 13_000_000;
    assert!(follows_up_at(until, line), "counted from the projected end");
    assert!(
        !follows_up_at(cut, line),
        "counted from the cut, it would not be"
    );
    said_until_us().store(before, Relaxed);
}

/// Whether `line`, begun `ago_s` s after the lamp's words to the euros
/// question ended (less than none: while they still play) and going on
/// `long_s` s before its end, was answered, in a room made busy first or
/// not, with `LAMP_NAME_OR_FOLLOW_UP` `on` or not.
async fn after_the_lamps_words(on: bool, busy: bool, ago_s: i64, long_s: u64, line: &str) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let _mark = OwnMark::kept();
    let _room = OwnRoom::fresh();
    if busy {
        // Three lines left alone inside a minute: people talking.
        let mut room = crate::room_told::room().lock().unwrap();
        for other in 901..=903 {
            room.unanswered(other, monotonic_us());
        }
        assert_eq!(room.at(monotonic_us()), crate::room_told::Room::Busy);
    }
    let before = name_or_follow_up().swap(on, Relaxed);
    let (mut rig, mut dial) = in_conversation(options(
        true,
        attention_now,
        crate::tools::Settings::default(),
    ))
    .await;
    // The words the worker counted ended `ago_s` s ago, or end later.
    said_until_us().store(
        monotonic_us().saturating_add_signed(-ago_s * 1_000_000),
        Relaxed,
    );
    let request = rig.speak(&mut dial, false, false).await;
    // The person goes on speaking, then ends.
    rig.pump(Duration::from_secs(long_s)).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation: rig.snapshot.microphone_generation(),
            sound: false,
            cut_in: false,
        })
        .unwrap();
    let end = rig.service_message(&mut dial).await;
    assert!(end["realtimeInput"].get("activityEnd").is_some(), "{end}");
    dial.service.send(heard(line)).await;
    dial.service.send(provider_audio(7, 240)).await;
    dial.service.send(idle_complete()).await;
    let mut played = false;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: of, .. } if of == request => played = true,
            ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
            _ => {}
        }
    }
    rig.stop().await;
    name_or_follow_up().store(before, Relaxed);
    said_until_us().store(0, Relaxed);
    played
}

/// Voice prints made at once from the line's sound: one voice says 1s,
/// another -1s.
pub(super) struct TwoVoices(
    pub(super) std::collections::VecDeque<(u64, std::result::Result<Vec<f32>, String>)>,
);

impl crate::same_voice::Printer for TwoVoices {
    fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
        let print = if samples[0] > 0 {
            vec![1.0, 0.0]
        } else {
            vec![0.0, 1.0]
        };
        self.0.push_back((request, Ok(print)));
        true
    }
    fn answer(&mut self) -> Option<(u64, std::result::Result<Vec<f32>, String>)> {
        self.0.pop_front()
    }
}

/// A line of `blocks` 10 ms blocks of `voice`, said with the lamp quiet or
/// over it, heard as `text` and answered: whether the answer's sound came
/// out of the worker.
async fn line(
    rig: &mut LiveRig,
    dial: &mut Dial,
    over_the_lamp: bool,
    voice: i16,
    blocks: u64,
    text: &str,
) -> bool {
    voice_line(rig, dial, over_the_lamp, voice, blocks, text, true).await
}

/// The same, answered by the service or, when not `answered`, left alone:
/// the worker then closes it as unanswered. Said over the lamp, its voice
/// could be heard.
pub(super) async fn voice_line(
    rig: &mut LiveRig,
    dial: &mut Dial,
    over_the_lamp: bool,
    voice: i16,
    blocks: u64,
    text: &str,
    answered: bool,
) -> bool {
    voice_line_over(
        rig,
        dial,
        (over_the_lamp, over_the_lamp),
        voice,
        blocks,
        text,
        answered,
    )
    .await
}

/// The same, begun with a reply in hand (`over.0`) whose sound could be
/// heard or not (`over.1`): `ProviderInput::Start`'s `over_the_lamp` and
/// `over_the_lamps_voice`.
async fn voice_line_over(
    rig: &mut LiveRig,
    dial: &mut Dial,
    over: (bool, bool),
    voice: i16,
    blocks: u64,
    text: &str,
    answered: bool,
) -> bool {
    let (over_the_lamp, over_the_lamps_voice) = over;
    let owner = rig
        .owner
        .admit(real_time(), lamp_turn_owner::AdmittedInput::NewTurn)
        .unwrap();
    let request = owner.turn();
    rig.publish();
    let privacy_generation = rig.snapshot.microphone_generation();
    rig.parent
        .data
        .send(ProviderInput::Start {
            session: dial.session.get(),
            request,
            privacy_generation,
            continues: false,
            continues_cut_off: false,
            over_the_lamp,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice,
            heard_of_cut: None,
            soft_lead_us: 0,
        })
        .unwrap();
    // Each block sent once the socket takes it, the coordinator's lease
    // kept fresh and what the worker sends the service read meanwhile, as a
    // coordinator and a service would: a worker left waiting on the service
    // stops reading its heartbeats.
    let deadline = Instant::now() + Duration::from_secs(5);
    for sequence in 1..=blocks {
        let audio = ProviderInput::Audio {
            request,
            privacy_generation,
            sequence,
            read_completed_at_us: monotonic_us(),
            samples: vec![voice; 160],
        };
        loop {
            assert!(
                Instant::now() < deadline,
                "block {sequence} was never taken"
            );
            while dial
                .service
                .try_receive(Duration::from_millis(1))
                .await
                .is_some()
            {}
            if rig.heartbeat().is_err() {
                continue;
            }
            match rig.parent.data.send(audio.clone()) {
                Ok(()) => break,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("{error}"),
            }
        }
    }
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Started { request: started, .. } if started == request
    ));
    rig.owner.input_ended(real_time(), owner).unwrap();
    rig.publish();
    rig.parent
        .data
        .send(ProviderInput::End {
            request,
            privacy_generation,
            sound: false,
            cut_in: false,
        })
        .unwrap();
    loop {
        let message = rig.service_message(dial).await;
        if message["realtimeInput"].get("activityEnd").is_some() {
            break;
        }
    }
    dial.service.send(heard(text)).await;
    if answered {
        dial.service.send(provider_audio(7, 240)).await;
        // The lamp's words, for the conversation the worker keeps.
        dial.service
            .send(json!({"serverContent":{"outputTranscription":{"text":"Sure."}}}))
            .await;
        dial.service.send(idle_complete()).await;
    }
    let mut played = false;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request: of, .. } if of == request => played = true,
            ProviderOutput::TurnComplete { request: of, .. } if of == request => break,
            _ => {}
        }
    }
    rig.owner.complete_turn(real_time(), owner).unwrap();
    rig.publish();
    played
}

/// The reviewer on #1030, the 06:07 scene with the model answering
/// Samantha: her "Anyway, …" in another voice is held, and counts as talk
/// between people; Daniel's "True, but …" is left alone; so his "Did you
/// get my message about the budget?" is no part of a conversation with the
/// lamp, and its answer is held. Without the check her line is answered,
/// and the budget line goes on with the conversation, as before.
///
/// On the logical clock (#1160): the worker has recorded the call's answer
/// in the conversation before the next line's answer is judged, the order
/// this scene means. In real time, at this test's pace, the two could fall
/// the other way (fourth on #1160).
#[tokio::test(start_paused = true)]
async fn with_the_switch_and_a_voice_check_a_line_held_as_another_voice_counts_as_talk() {
    let _clock = logical_clock();
    const CALL: &str = "Hey Lamp, how much is one hundred dollars in euros?";
    const SAMANTHA: &str = "Anyway, if we drop the trip we save almost half.";
    // Not "True, but …": the rule for whom a line answers takes "True," for
    // a name, so the budget line would be held as a reply to "True" (fixed
    // with the other openers on #1090).
    const DANIEL: &str = "But the client wants to meet in person.";
    let scene = |check: bool| async move {
        let _mark = OwnMark::kept();
        let _room = OwnRoom::fresh();
        let mut options = options(true, attention_now, crate::tools::Settings::default());
        if check {
            options.voices = Some(Box::new(TwoVoices(Default::default())));
        }
        let mut rig = LiveRig::start_configured(
            quick_policy(),
            options,
            BARRIER,
            false,
            Continuation::Off,
            lamp_gemini::MAX_WIRE_BYTES * 2,
            |config| {
                config
                    .timeouts(lamp_gemini::Timeouts {
                        unanswered: Some(Duration::from_millis(100)),
                        ..Default::default()
                    })
                    .unwrap()
            },
        );
        let mut dial = rig.ready().await;
        assert!(voice_line(&mut rig, &mut dial, false, 1, 150, CALL, true).await);
        let samantha = voice_line(&mut rig, &mut dial, false, -1, 150, SAMANTHA, true).await;
        assert!(!voice_line(&mut rig, &mut dial, false, 1, 150, DANIEL, false).await);
        let budget = voice_line(&mut rig, &mut dial, false, 1, 150, NOT_TO_THE_LAMP[0], true).await;
        rig.stop().await;
        (samantha, budget)
    };
    assert_eq!(scene(true).await, (false, false));
    assert_eq!(scene(false).await, (true, true));
}

/// #829, lamp-4ace on 19 October (queue 22e): Samantha's "Pepper", held as
/// another voice, ended Daniel's conversation, and his next questions to the
/// lamp were held: "What about a name for a black kitten then?" (its
/// "Lamp," not heard as the name) and "And which of those names is more
/// popular?". So was scene 4's "And how long would that take by train?"
/// after a television's line. A question in the conversation's own voice
/// now goes on with it after only another voice's line; her own question
/// there, and her line after the lamp's answer, are still held. A statement
/// in his voice there is his talk to her, which ends the conversation, so
/// his question after it is held too, whether or not the model answered
/// the statement (the reviewer on #1030 and on #1636).
#[tokio::test(start_paused = true)]
async fn with_a_voice_check_a_question_in_the_conversations_voice_goes_on_after_another_voices_line()
 {
    let _clock = logical_clock();
    let scene = |lines: Vec<(i16, &'static str)>| async move {
        let _mark = OwnMark::kept();
        let _room = OwnRoom::fresh();
        let mut options = options(true, attention_now, crate::tools::Settings::default());
        options.voices = Some(Box::new(TwoVoices(Default::default())));
        let mut rig = LiveRig::start_configured(
            quick_policy(),
            options,
            BARRIER,
            false,
            Continuation::Off,
            lamp_gemini::MAX_WIRE_BYTES * 2,
            |config| {
                config
                    .timeouts(lamp_gemini::Timeouts {
                        unanswered: Some(Duration::from_millis(100)),
                        ..Default::default()
                    })
                    .unwrap()
            },
        );
        let mut dial = rig.ready().await;
        let mut played = Vec::new();
        for (voice, text) in lines {
            played.push(voice_line(&mut rig, &mut dial, false, voice, 150, text, true).await);
        }
        rig.stop().await;
        played
    };
    const DANIEL: i16 = 1;
    const SAMANTHA: i16 = -1;
    // Scene 5, her own question after "Pepper", and her line after the
    // lamp's answer.
    assert_eq!(
        scene(vec![
            (
                DANIEL,
                "Hey Lamp, what is a good name for a small grey cat?"
            ),
            (
                SAMANTHA,
                "I think we should call her Pepper, she looks like one."
            ),
            (SAMANTHA, "Do you like that name?"),
            (DANIEL, "What about a name for a black kitten then?"),
            (DANIEL, "And which of those names is more popular?"),
            (SAMANTHA, "I still like Pepper better."),
        ])
        .await,
        [true, false, false, true, true, false]
    );
    // Scene 4: a television's line, then his question.
    assert_eq!(
        scene(vec![
            (DANIEL, "Hey Lamp, how far is it from Hanoi to Da Nang?"),
            (
                SAMANTHA,
                "In other news, heavy rain is expected across the north this weekend."
            ),
            (DANIEL, "And how long would that take by train?"),
        ])
        .await,
        [true, false, true]
    );
    // A statement in his voice after her line is his talk to her, whatever
    // "you", "let's" or "thanks" is in it.
    for statement in [
        "Yes, and we should leave a bit early.",
        "Yes, and you should bring yours too.",
        "Okay, let's leave a bit early then.",
        "Thanks, I'll take the car.",
    ] {
        assert_eq!(
            scene(vec![
                (DANIEL, "Hey Lamp, what is the weather like in Hanoi today?"),
                (SAMANTHA, "I should bring an umbrella then."),
                (DANIEL, statement),
            ])
            .await,
            [true, false, false],
            "{statement}"
        );
    }
    // #1030's 06:07 scene with the model answering Daniel's statement: held
    // by the rule, it still ends the conversation, and his "budget"
    // question after it is held.
    assert_eq!(
        scene(vec![
            (
                DANIEL,
                "Hey Lamp, how much is one hundred dollars in euros?"
            ),
            (SAMANTHA, "Anyway, if we drop the trip we save almost half."),
            (DANIEL, "But the client wants to meet in person."),
            (DANIEL, "Did you get my message about the budget?"),
        ])
        .await,
        [true, false, false, false]
    );
}

/// Voice prints as [`TwoVoices`] makes them, but the other voice's at a
/// cosine of 0.35 against the first's: under #829's old bar of 0.45, over
/// the one bar of 0.30 (`DIFFERENT_BELOW`).
struct NearVoices(std::collections::VecDeque<(u64, std::result::Result<Vec<f32>, String>)>);

impl crate::same_voice::Printer for NearVoices {
    fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
        let print = if samples[0] > 0 {
            vec![1.0, 0.0]
        } else {
            vec![0.35, 0.937]
        };
        self.0.push_back((request, Ok(print)));
        true
    }
    fn answer(&mut self) -> Option<(u64, std::result::Result<Vec<f32>, String>)> {
        self.0.pop_front()
    }
}

/// One bar for the voice, 0.30 (`DIFFERENT_BELOW`; the first builder at
/// 03:50 on Queue #1722), through the worker's own loop: after A's call, an
/// unnamed follow-up in a voice at 0.35 against A's, held as another voice's
/// under #829's old bar of 0.45, is the conversation's and answered, as A's
/// own follow-up is. On lamp-4ace's mic2 one voice in a quiet room read
/// down to 0.35 against itself (`docs/verification/same-voice-20261021`).
/// On the logical clock, as its sibling above is.
#[tokio::test(start_paused = true)]
async fn with_the_switch_and_a_voice_check_a_voice_at_0_35_held_at_0_45_is_answered_at_0_30() {
    let _clock = logical_clock();
    const CALL: &str = "Hey Lamp, what is the weather like in Hanoi today?";
    const FOLLOW_UP: &str = "And what about tomorrow?";
    let follow_up_by = |voice: i16| async move {
        let _mark = OwnMark::kept();
        let _room = OwnRoom::fresh();
        let mut options = options(true, attention_now, crate::tools::Settings::default());
        options.voices = Some(Box::new(NearVoices(Default::default())));
        let mut rig = LiveRig::start_configured(
            quick_policy(),
            options,
            BARRIER,
            false,
            Continuation::Off,
            lamp_gemini::MAX_WIRE_BYTES * 2,
            |config| {
                config
                    .timeouts(lamp_gemini::Timeouts {
                        unanswered: Some(Duration::from_millis(100)),
                        ..Default::default()
                    })
                    .unwrap()
            },
        );
        let mut dial = rig.ready().await;
        assert!(voice_line(&mut rig, &mut dial, false, 1, 150, CALL, true).await);
        let played = voice_line(&mut rig, &mut dial, false, voice, 150, FOLLOW_UP, true).await;
        rig.stop().await;
        played
    };
    assert!(follow_up_by(-1).await);
    assert!(follow_up_by(1).await);
}

/// #829, the battery's two-people scene 3 on lamp-4ace (17 October), line by
/// line through the worker's own loop, with the room and the conversation as
/// the worker keeps them. Each line is as long as the lamp sends it: its
/// `say` length with the coordinator's 0.3 s pre-roll before it and the
/// product's 0.6 s of end silence after it (the reviewer on #1415), so B's
/// short "And what about tomorrow?" (1.19 s alone) is long enough to be
/// judged. A asks the lamp by name and is answered; B's lines are held as
/// another voice; and one such line ends the conversation, so A's own remark
/// to B after it, which the service would answer, is held. With no voice
/// check every line is played, as all seven of that evening's batteries
/// played the umbrella line. On the paused clock, as its sibling above is:
/// in real time the call's answer and the next line's verdict could fall
/// the other way (the Seventh on #1415).
#[tokio::test(start_paused = true)]
async fn with_the_switch_and_a_voice_check_another_voice_ends_the_conversation() {
    let _clock = logical_clock();
    const CALL: &str = "Hey Lamp, what is the weather like in Hanoi today?";
    const FOLLOW_UP: &str = "And what about tomorrow?";
    const UMBRELLA: &str = "I should bring an umbrella then.";
    const EARLY: &str = "Yes, and we should leave a bit early.";
    const CAR: &str = "Okay, I will book the car for eight.";
    // The pre-roll and the end silence, in 10 ms blocks.
    const SENT: u64 = 30 + 60;
    let scene = |check: bool, lines: Vec<(i16, u64, &'static str)>| async move {
        let _mark = OwnMark::kept();
        let _room = OwnRoom::fresh();
        let mut options = options(true, attention_now, crate::tools::Settings::default());
        if check {
            options.voices = Some(Box::new(TwoVoices(Default::default())));
        }
        let mut rig = LiveRig::start_configured(
            quick_policy(),
            options,
            BARRIER,
            false,
            Continuation::Off,
            lamp_gemini::MAX_WIRE_BYTES * 2,
            |config| {
                config
                    .timeouts(lamp_gemini::Timeouts {
                        unanswered: Some(Duration::from_millis(100)),
                        ..Default::default()
                    })
                    .unwrap()
            },
        );
        let mut dial = rig.ready().await;
        let mut played = Vec::new();
        for (voice, blocks, text) in lines {
            played.push(
                voice_line(&mut rig, &mut dial, false, voice, SENT + blocks, text, true).await,
            );
        }
        rig.stop().await;
        played
    };
    // A is voice 1, B voice -1; each line's own length in 10 ms blocks is
    // what `say` gives it in the battery's voices.
    let scene_3 = || {
        vec![
            (1, 295, CALL),
            (-1, 119, FOLLOW_UP),
            (-1, 158, UMBRELLA),
            (1, 222, EARLY),
            (-1, 214, CAR),
        ]
    };
    assert_eq!(
        scene(true, scene_3()).await,
        [true, false, false, false, false]
    );
    assert_eq!(scene(false, scene_3()).await, [true; 5]);
    // One line of B's between: the two-lines rule alone would leave A's
    // remark to B in the conversation; one line held as another voice ends it.
    let one_between = || vec![(1, 295, CALL), (-1, 158, UMBRELLA), (1, 222, EARLY)];
    assert_eq!(scene(true, one_between()).await, [true, false, false]);
    assert_eq!(scene(false, one_between()).await, [true; 3]);
}

/// Voice prints as [`TwoVoices`] makes them, but late for a line asked for
/// at its end: a line asked for while it is heard, at its first
/// [`crate::same_voice::ASKED_AT`], is printed at once, as A's call, asked
/// for while heard, was on lamp-4ace. One shorter, asked for at its end, is
/// answered on the second look, after its answer's verdict, as the child on
/// lamp-4ace answered requests 18 and 19 on 18 October (#829, the first
/// builder at 06:54). With `every_line`, every line is late so, as on a
/// lamp slower still.
#[derive(Default)]
struct LatePrints {
    every_line: bool,
    ready: std::collections::VecDeque<(u64, std::result::Result<Vec<f32>, String>)>,
    late: std::collections::VecDeque<(u64, std::result::Result<Vec<f32>, String>, bool)>,
}

impl crate::same_voice::Printer for LatePrints {
    fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
        let print = if samples[0] > 0 {
            vec![1.0, 0.0]
        } else {
            vec![0.0, 1.0]
        };
        if samples.len() == crate::same_voice::ASKED_AT && !self.every_line {
            self.ready.push_back((request, Ok(print)));
        } else {
            self.late.push_back((request, Ok(print), false));
        }
        true
    }
    fn answer(&mut self) -> Option<(u64, std::result::Result<Vec<f32>, String>)> {
        if let Some(ready) = self.ready.pop_front() {
            return Some(ready);
        }
        match self.late.front_mut() {
            Some((_, _, looked)) if !*looked => {
                *looked = true;
                None
            }
            Some(_) => self
                .late
                .pop_front()
                .map(|(request, print, _)| (request, print)),
            None => None,
        }
    }
}

/// #829, lamp-4ace on 18 October at `6ea956900` with the check on: scene 3
/// at the lamp's lengths, with the prints as the lamp's child gave them: in
/// time for a line asked for while it is heard, after the verdict for one
/// asked for at its end. Every line of the scene is 1.8 s or more as sent,
/// so each is asked for while it is heard and judged with the check: A's
/// call is answered, and B's follow-up, in another voice, is held and ends
/// the conversation, as
/// `with_the_switch_and_a_voice_check_another_voice_ends_the_conversation`
/// says with every print in time. Asked for at 3 s, as on the lamp, B's
/// follow-up and umbrella line were asked for only at their ends, judged
/// without the check and played (`[true, true, true, false, false]`).
#[tokio::test(start_paused = true)]
async fn with_a_voice_check_whose_prints_take_the_lamps_time_the_scene_is_judged_with_it() {
    let _clock = logical_clock();
    assert_eq!(
        scene_3_with(LatePrints::default()).await,
        [true, false, false, false, false]
    );
}

/// The Seventh on #1445: on a lamp slower still, with every print after
/// its line's verdict, B's follow-up is let out as the conversation going
/// on, and its print, come late, is another voice than A's: the
/// conversation is over, as for one held, and the umbrella line, A's
/// remark and the car line are held. A late print used to become the
/// conversation's voice unchecked, and the scene then went on with B:
/// every line was played.
#[tokio::test(start_paused = true)]
async fn with_a_voice_check_whose_every_print_comes_late_a_late_other_voice_ends_the_conversation()
{
    let _clock = logical_clock();
    let every_line = LatePrints {
        every_line: true,
        ..LatePrints::default()
    };
    assert_eq!(
        scene_3_with(every_line).await,
        [true, true, false, false, false]
    );
}

/// Scene 3 at the lamp's lengths through the worker's own loop, with the
/// prints as `prints` gives them: which of its five lines were played.
async fn scene_3_with(prints: LatePrints) -> Vec<bool> {
    scene_with(
        prints,
        &[
            (1, 295, "Hey Lamp, what is the weather like in Hanoi today?"),
            (-1, 119, "And what about tomorrow?"),
            (-1, 158, "I should bring an umbrella then."),
            (1, 222, "Yes, and we should leave a bit early."),
            (-1, 214, "Okay, I will book the car for eight."),
        ],
    )
    .await
}

/// Eighth on #829: on a lamp where every print comes after its verdict, B
/// calls the lamp and follows up, and then A calls it by name while B's
/// conversation is still going on. A's call begins a conversation of its
/// own, so A's late print is no part of B's, and A's follow-up is
/// answered. Taken as B's conversation going on, A's late print was
/// another voice's than B's, and it ended A's conversation at A's next
/// line.
#[tokio::test(start_paused = true)]
async fn with_a_voice_check_a_named_call_amid_anothers_conversation_begins_its_own() {
    let _clock = logical_clock();
    let every_line = LatePrints {
        every_line: true,
        ..LatePrints::default()
    };
    let played = scene_with(
        every_line,
        &[
            (-1, 250, "Hey Lamp, what time is it in Tokyo?"),
            (-1, 200, "And what is the date there?"),
            (1, 295, "Hey Lamp, what is the weather like in Hanoi today?"),
            (1, 200, "And what about tomorrow?"),
        ],
    )
    .await;
    assert_eq!(played, [true; 4]);
}

/// Lines (voice, `say` length in 10 ms blocks, words) at the lamp's
/// lengths through the worker's own loop, with the prints as `prints`
/// gives them: which of them were played.
async fn scene_with(prints: LatePrints, lines: &[(i16, u64, &str)]) -> Vec<bool> {
    let _mark = OwnMark::kept();
    let _room = OwnRoom::fresh();
    let mut options = options(true, attention_now, crate::tools::Settings::default());
    options.voices = Some(Box::new(prints));
    let mut rig = LiveRig::start_configured(
        quick_policy(),
        options,
        BARRIER,
        false,
        Continuation::Off,
        lamp_gemini::MAX_WIRE_BYTES * 2,
        |config| {
            config
                .timeouts(lamp_gemini::Timeouts {
                    unanswered: Some(Duration::from_millis(100)),
                    ..Default::default()
                })
                .unwrap()
        },
    );
    let mut dial = rig.ready().await;
    // The pre-roll and the end silence, in 10 ms blocks, around each line's
    // `say` length.
    const SENT: u64 = 30 + 60;
    let mut played = Vec::new();
    for &(voice, blocks, text) in lines {
        played.push(voice_line(&mut rig, &mut dial, false, voice, SENT + blocks, text, true).await);
    }
    rig.stop().await;
    played
}

/// The reviewer on #1636 (03:41): #1030's 06:07 scene with the prints as
/// lamp-4ace's child gives them. Daniel's short "But the client wants to
/// meet in person.", after Samantha's line was held as another voice, is
/// printed only at its end, after its verdict. It still ends the
/// conversation, so his "budget" question after it is held.
#[tokio::test(start_paused = true)]
async fn with_a_voice_check_a_statement_whose_print_is_late_after_another_voices_line_ends_the_conversation()
 {
    let _clock = logical_clock();
    assert_eq!(
        scene_with(
            LatePrints::default(),
            &[
                (
                    1,
                    295,
                    "Hey Lamp, how much is one hundred dollars in euros?"
                ),
                (-1, 200, "Anyway, if we drop the trip we save almost half."),
                (1, 70, "But the client wants to meet in person."),
                (1, 200, "Did you get my message about the budget?"),
            ]
        )
        .await,
        [true, false, false, false]
    );
}

/// Beside the test above: a short question in the conversation's voice,
/// printed only after its verdict, is held for want of its print, but it is
/// no talk past the lamp. The conversation goes on for the question after
/// it, printed in time.
#[tokio::test(start_paused = true)]
async fn with_a_voice_check_a_question_whose_print_is_late_after_another_voices_line_leaves_the_conversation_going_on()
 {
    let _clock = logical_clock();
    assert_eq!(
        scene_with(
            LatePrints::default(),
            &[
                (
                    1,
                    295,
                    "Hey Lamp, how much is one hundred dollars in euros?"
                ),
                (-1, 200, "Anyway, if we drop the trip we save almost half."),
                (1, 70, "Is that a lot of money?"),
                (1, 200, "And how much is that in Japanese yen?"),
            ]
        )
        .await,
        [true, false, false, true]
    );
}

/// #829, through the worker's own loop, with a conversation going on: after
/// Daniel's call is answered, a line of 1.5 s in another voice is held, and
/// one in his is played. Said over the lamp, or with no voice check, the
/// other voice is played as before. On the paused clock: the rig's leases
/// are 100 ms, so on the host's clock a stall of the test's thread past
/// that, between a heartbeat and the line's end, revoked the turn
/// (`StaleOwner` at `input_ended`, the first builder's emulated ARM64 run of
/// `6ea956900`).
#[tokio::test(start_paused = true)]
async fn with_the_switch_and_a_voice_check_another_voice_is_no_part_of_the_conversation() {
    let _clock = logical_clock();
    const CALL: &str = "Hey Lamp, how much is one hundred dollars in euros?";
    const SAMANTHA: &str = "Anyway, if we drop the trip we save almost half.";
    let scene = |check: bool, over_the_lamp: bool, voice: i16| async move {
        let mut options = options(true, quiet_going_on, crate::tools::Settings::default());
        if check {
            options.voices = Some(Box::new(TwoVoices(Default::default())));
        }
        let mut rig = LiveRig::start_with(quick_policy(), options);
        let mut dial = rig.ready().await;
        assert!(line(&mut rig, &mut dial, false, 1, 150, CALL).await);
        let played = line(&mut rig, &mut dial, over_the_lamp, voice, 150, SAMANTHA).await;
        rig.stop().await;
        played
    };
    assert!(!scene(true, false, -1).await, "another voice was played");
    assert!(scene(true, false, 1).await, "the same voice was held");
    assert!(
        scene(true, true, -1).await,
        "a line over the lamp was judged"
    );
    assert!(
        scene(false, false, -1).await,
        "without the check it was held"
    );
}

/// #829, lamp-4ace on 18 October (queue 15): each scene's call came while
/// the line before it, left unanswered, still had its turn open, so it was
/// admitted with a reply in hand that made no sound. It was never printed,
/// and the conversation took the next speaker's voice: Samantha's "And what
/// about tomorrow?" after Daniel's call, then her umbrella line as the
/// conversation's own; in scene 5 her "Pepper", then Daniel's own "which of
/// those names is more popular?" held as another voice. A call begun over a
/// silent reply is printed now, and the scenes go as the first builder's
/// table has them. A line begun while the lamp could be heard is still
/// never judged.
#[tokio::test(start_paused = true)]
async fn with_a_voice_check_a_call_begun_over_a_silent_reply_gives_the_conversation_its_voice() {
    let _clock = logical_clock();
    // Begun with nothing in hand, over a silent reply in hand, and over the
    // lamp's voice: `ProviderInput::Start`'s two marks.
    const CLEAR: (bool, bool) = (false, false);
    const SILENT: (bool, bool) = (true, false);
    const VOICE: (bool, bool) = (true, true);
    let scene_with = |check: bool, lines: &'static [((bool, bool), i16, &'static str)]| async move {
        let mut options = options(true, quiet_going_on, crate::tools::Settings::default());
        if check {
            options.voices = Some(Box::new(TwoVoices(Default::default())));
        }
        let mut rig = LiveRig::start_with(quick_policy(), options);
        let mut dial = rig.ready().await;
        let mut played = Vec::new();
        for &(over, voice, text) in lines {
            played.push(voice_line_over(&mut rig, &mut dial, over, voice, 150, text, true).await);
        }
        rig.stop().await;
        played
    };
    let scene = |lines| scene_with(true, lines);
    // Scene 1: Daniel calls, Samantha follows up, then remarks.
    assert_eq!(
        scene(&[
            (
                SILENT,
                1,
                "Hey Lamp, what is the weather like in Hanoi today?"
            ),
            (CLEAR, -1, "And what about tomorrow?"),
            (CLEAR, -1, "I should bring an umbrella then."),
        ])
        .await,
        [true, false, false]
    );
    // Without the check (the reviewer on #1627): scene 1 as before, every
    // line answered.
    assert_eq!(
        scene_with(
            false,
            &[
                (
                    SILENT,
                    1,
                    "Hey Lamp, what is the weather like in Hanoi today?"
                ),
                (CLEAR, -1, "And what about tomorrow?"),
                (CLEAR, -1, "I should bring an umbrella then."),
            ]
        )
        .await,
        [true, true, true]
    );
    // Scene 5: Samantha's "Pepper" is another voice; Daniel calls again,
    // over its silent reply in hand, and his own follow-up is his.
    assert_eq!(
        scene(&[
            (
                SILENT,
                1,
                "Hey Lamp, what is a good name for a small grey cat?"
            ),
            (
                CLEAR,
                -1,
                "I think we should call her Pepper, she looks like one."
            ),
            (
                SILENT,
                1,
                "Lamp, what about a name for a black kitten then?"
            ),
            (CLEAR, 1, "And which of those names is more popular?"),
        ])
        .await,
        [true, false, true, true]
    );
    // The caller's own follow-up after a call begun over a silent reply.
    assert_eq!(
        scene(&[
            (
                SILENT,
                1,
                "Hey Lamp, what is the weather like in Hanoi today?"
            ),
            (CLEAR, 1, "And what about tomorrow?"),
        ])
        .await,
        [true, true]
    );
    // Over the lamp's own voice the call is still never printed, and the
    // line after it is judged as without the check.
    assert_eq!(
        scene(&[
            (
                VOICE,
                1,
                "Hey Lamp, what is the weather like in Hanoi today?"
            ),
            (CLEAR, -1, "And what about tomorrow?"),
        ])
        .await,
        [true, true]
    );
}

/// A real `lamp-same-voice` child held before Ready. Its private FIFO
/// probes acknowledge the same live child throughout the conversation.
struct LoadingChild {
    dir: std::path::PathBuf,
    prints: std::sync::Arc<std::sync::Mutex<Option<crate::same_voice::Prints>>>,
    control: Option<std::fs::File>,
    pid: Option<rustix::process::Pid>,
    probe: u64,
    launched: bool,
    finished: bool,
}

/// Let cleanup close Prints even if a panicking paused test has not yet
/// dropped the worker task. The real Printer still handles every call.
struct LoadingPrints(std::sync::Arc<std::sync::Mutex<Option<crate::same_voice::Prints>>>);

impl crate::same_voice::Printer for LoadingPrints {
    fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
        self.0
            .lock()
            .unwrap()
            .as_mut()
            .is_some_and(|prints| crate::same_voice::Printer::ask(prints, request, samples))
    }

    fn answer(&mut self) -> Option<(u64, std::result::Result<Vec<f32>, String>)> {
        self.0
            .lock()
            .unwrap()
            .as_mut()
            .and_then(crate::same_voice::Printer::answer)
    }
}

impl LoadingChild {
    const HOST_WAIT: Duration = Duration::from_secs(5);

    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "lamp-same-voice-loading-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join(crate::same_voice::EXECUTABLE);
        std::fs::write(
            &path,
            "#!/bin/sh\n\
             dir=${0%/*}\n\
             printf '%s\\n' \"$$\" > \"$dir/pid\" || exit 71\n\
             [ ! -e \"$dir/abort\" ] || exit 0\n\
             mkfifo \"$dir/control\" || exit 72\n\
             exec 3< \"$dir/control\" || exit 73\n\
             while IFS= read -r probe <&3; do\n\
                 printf '%s %s\\n' \"$probe\" \"$$\" > \"$dir/held\" || exit 74\n\
             done\n\
             printf 'released %s\\n' \"$$\" > \"$dir/released\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            dir,
            prints: std::sync::Arc::default(),
            control: None,
            pid: None,
            probe: 0,
            launched: false,
            finished: false,
        }
    }

    fn printer(&mut self) -> Box<dyn crate::same_voice::Printer> {
        *self.prints.lock().unwrap() = Some(crate::same_voice::Prints::start(
            self.dir.join(crate::same_voice::EXECUTABLE),
            "model".into(),
        ));
        self.launched = true;
        Box::new(LoadingPrints(self.prints.clone()))
    }

    fn pending(deadline: std::time::Instant, what: &str) -> std::io::Result<()> {
        if std::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, what));
        }
        // The runtime is paused: this is a host watchdog, never a Tokio wait.
        std::thread::sleep(Duration::from_millis(1));
        Ok(())
    }

    fn pid(&mut self, deadline: std::time::Instant) -> std::io::Result<rustix::process::Pid> {
        while self.pid.is_none() {
            match std::fs::read_to_string(self.dir.join("pid")) {
                Ok(text) => {
                    if let Some(complete) = text.strip_suffix('\n') {
                        self.pid = complete
                            .parse::<i32>()
                            .ok()
                            .filter(|pid| *pid > 0)
                            .and_then(rustix::process::Pid::from_raw);
                        if self.pid.is_none() {
                            return Err(std::io::Error::other("invalid loading child PID"));
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            if self.pid.is_none() {
                Self::pending(deadline, "loading child did not publish its complete PID")?;
            }
        }
        Ok(self.pid.unwrap())
    }

    fn open_control(&self) -> std::io::Result<Option<std::fs::File>> {
        use rustix::fs::{Mode, OFlags, open};
        match open(
            self.dir.join("control"),
            OFlags::WRONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => Ok(Some(fd.into())),
            Err(rustix::io::Errno::NOENT | rustix::io::Errno::NXIO) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn held(&mut self) -> std::io::Result<()> {
        use rustix::process::{WaitId, WaitIdOptions, waitid};
        use std::io::Write;
        let deadline = std::time::Instant::now() + Self::HOST_WAIT;
        let pid = self.pid(deadline)?;
        while self.control.is_none() {
            self.control = self.open_control()?;
            if self.control.is_none() {
                Self::pending(deadline, "loading child did not open its hold FIFO")?;
            }
        }
        self.probe += 1;
        writeln!(self.control.as_mut().unwrap(), "{}", self.probe)?;
        let expected = format!("{} {}\n", self.probe, pid.as_raw_nonzero());
        loop {
            let state = waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            )?;
            if state.is_some() {
                return Err(std::io::Error::other("loading child exited before release"));
            }
            match std::fs::read_to_string(self.dir.join("held")) {
                Ok(text) if text == expected => return Ok(()),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            Self::pending(deadline, "loading child did not acknowledge its held probe")?;
        }
    }

    fn finish(&mut self) -> std::io::Result<()> {
        use rustix::process::{WaitId, WaitIdOptions, waitid};
        self.finished = true;
        // Close the real request sender before releasing the child, including
        // on unwind: the supervisor must exit rather than start another one.
        self.prints.lock().unwrap_or_else(|p| p.into_inner()).take();
        let mut released = self.control.take().is_some();
        if self.launched {
            // A start that missed the test's watchdog must not later block.
            std::fs::write(self.dir.join("abort"), b"")?;
            let deadline = std::time::Instant::now() + Self::HOST_WAIT;
            let pid = self.pid(deadline)?;
            loop {
                match waitid(
                    WaitId::Pid(pid),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                ) {
                    Err(rustix::io::Errno::CHILD) => break,
                    Ok(None) if !released => released = self.open_control()?.is_some(),
                    Ok(_) | Err(rustix::io::Errno::INTR) => {}
                    Err(error) => return Err(error.into()),
                }
                Self::pending(deadline, "loading child was not reaped by its supervisor")?;
            }
            if self.probe != 0
                && std::fs::read_to_string(self.dir.join("released"))?
                    != format!("released {}\n", pid.as_raw_nonzero())
            {
                return Err(std::io::Error::other(
                    "loading child did not acknowledge release",
                ));
            }
        }
        std::fs::remove_dir_all(&self.dir)
    }
}

impl Drop for LoadingChild {
    fn drop(&mut self) {
        if !self.finished
            && let Err(error) = self.finish()
        {
            // Retain the script if startup or reaping was not proved; removing
            // it could race the asynchronous launch. Never mask an unwind.
            if std::thread::panicking() {
                eprintln!(
                    "loading child cleanup failed at {}: {error}",
                    self.dir.display()
                );
            } else {
                panic!(
                    "loading child cleanup failed at {}: {error}",
                    self.dir.display()
                );
            }
        }
    }
}

/// The first builder on #1445: while the same-voice child loads its model
/// (15.5 s on lamp-4ace), the worker's loop goes on. Here a real child
/// process never gets ready: each line is taken, the coordinator's
/// heartbeats are read, and every line is answered as without the check,
/// another voice's too. A worker that waited on the child would never take
/// a block (`voice_line`'s "was never taken").
#[tokio::test(start_paused = true)]
async fn a_child_still_loading_its_model_takes_nothing_from_the_worker() {
    let _one = one_thread();
    let _clock = logical_clock();
    let mut child = LoadingChild::new();
    let mut options = options(true, quiet_going_on, crate::tools::Settings::default());
    options.voices = Some(child.printer());
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    child.held().unwrap();
    for (voice, text) in [
        (1, "Hey Lamp, how much is one hundred dollars in euros?"),
        (-1, "Anyway, if we drop the trip we save almost half."),
        (1, "And what about in yen?"),
    ] {
        assert!(
            line(&mut rig, &mut dial, false, voice, 300, text).await,
            "{text}"
        );
        child.held().unwrap();
    }
    rig.stop().await;
    child.finish().unwrap();
}

/// What the model does with Samantha's "Yes, give me five minutes.", let
/// out as the conversation going on after one line left alone.
#[derive(Clone, Copy, Debug)]
enum Answer {
    /// Turns the volume up, and says nothing.
    Done,
    /// The same, and the Policy judges the turn not the lamp's.
    PolicyHeld,
    /// Asks the time, then says the line was not for it.
    LookedUpThenNotForMe,
}

/// The sixth's case on #1029 and the reviewer's two on #1069: after the
/// euros answer and one line left alone, Samantha's "Yes, give me five
/// minutes." is answered as `answer` says; then Daniel's "Did you get my
/// message about the budget?". Whether its answer was played, and whether
/// the volume was turned up.
async fn after_a_line_answered_by(answer: Answer) -> (bool, bool) {
    let _mark = OwnMark::kept();
    let _room = OwnRoom::fresh();
    let mut desk = super::policy_holds::Desk::new("sto");
    let mut with = desk.options(true);
    with.tools.decline = true;
    with.tools.clock = true;
    with.attention = Some(attention_now);
    with.spoken_to = true;
    let (mut rig, mut dial) = in_conversation(with).await;
    left_alone(
        &mut rig,
        &mut dial,
        "Anyway, if we drop the trip we save almost half.",
    )
    .await;
    let five = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard("Yes, give me five minutes.")).await;
    match answer {
        Answer::Done | Answer::PolicyHeld => {
            let choice = match answer {
                Answer::PolicyHeld => "stay_quiet",
                _ => "answer",
            };
            desk.decide(Some(five), choice);
            dial.service.send(function_call("c1", "volume_up")).await;
            rig.service_message(&mut dial).await;
        }
        Answer::LookedUpThenNotForMe => {
            for (id, name) in [
                ("c1", crate::tools::THE_TIME),
                ("c2", crate::tools::NOT_FOR_ME),
            ] {
                dial.service.send(function_call(id, name)).await;
                rig.service_message(&mut dial).await;
            }
        }
    }
    // The asking turn's end, then the turn's: the model says nothing more.
    for _ in 0..2 {
        dial.service.send(idle_complete()).await;
    }
    ended_unplayed(&mut rig, five, "Yes, give me five minutes.").await;
    complete(&mut rig);
    let played = played(&mut rig, &mut dial, NOT_TO_THE_LAMP[0]).await;
    rig.stop().await;
    let turned_up = desk.asked.try_recv().is_ok();
    (played, turned_up)
}

/// A line answered by something done, with no word said, is answered: the
/// conversation goes on (the sixth on #1029: "lights off", then
/// "brighter").
#[tokio::test]
async fn with_the_switch_a_line_answered_by_an_action_alone_keeps_the_conversation() {
    assert_eq!(after_a_line_answered_by(Answer::Done).await, (true, true));
}

/// An action the Policy held is not an answer: the line was left alone,
/// the second since the lamp spoke, and the conversation has ended (the
/// reviewer on #1069 at 18:08).
#[tokio::test]
async fn with_the_switch_an_action_the_policy_held_is_no_answer() {
    assert_eq!(
        after_a_line_answered_by(Answer::PolicyHeld).await,
        (false, false)
    );
}

/// Nor is a lookup followed by "not for me" (the reviewer on #1069 at
/// 21:10).
#[tokio::test]
async fn with_the_switch_a_lookup_then_not_for_me_is_no_answer() {
    assert_eq!(
        after_a_line_answered_by(Answer::LookedUpThenNotForMe).await,
        (false, false)
    );
}
