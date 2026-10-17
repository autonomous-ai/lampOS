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
        tools,
        attention: Some(room),
        name_signal: false,
        sounds_checked: false,
        name_in_passing_ignored: false,
        policy_holds: None,
        wait_for_room: false,
        staff_note: false,
        ask_unclear: false,
        spoken_to,
        voices: None,
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
    /// of the six (the eighth, 01:0x on #705).
    _one: std::sync::MutexGuard<'static, ()>,
}

/// One test at a time holds the process's thread.
static ONE_THREAD: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl OwnRoom {
    pub(super) fn fresh() -> Self {
        let one = ONE_THREAD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

/// Voice prints made at once from the line's sound: one voice says 1s,
/// another -1s.
struct TwoVoices(std::collections::VecDeque<(u64, std::result::Result<Vec<f32>, String>)>);

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
/// the worker then closes it as unanswered.
async fn voice_line(
    rig: &mut LiveRig,
    dial: &mut Dial,
    over_the_lamp: bool,
    voice: i16,
    blocks: u64,
    text: &str,
    answered: bool,
) -> bool {
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
            over_the_lamps_voice: false,
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

/// #829, through the worker's own loop, with a conversation going on: after
/// Daniel's call is answered, a line of 1.5 s in another voice is held, and
/// one in his is played. Said over the lamp, or with no voice check, the
/// other voice is played as before.
#[tokio::test]
async fn with_the_switch_and_a_voice_check_another_voice_is_no_part_of_the_conversation() {
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
