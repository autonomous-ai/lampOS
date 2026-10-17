//! The Policy holds the lamp's actions (#800, `LAMP_POLICY_HOLDS_ACTIONS=on`),
//! through the worker's own loop against the scripted service, an executive
//! that answers every call and a bus of the test's own: an action in a turn
//! the Policy judged `stay_quiet` is answered, not run, and none of the
//! turn's sound comes out; one in a turn it took as the lamp's runs; with no
//! decision about its own turn it runs after the wait; and without the
//! switch it runs at once.
use super::*;
use std::io::{BufRead, Write};

/// An executive that answers every call done, telling the test each line
/// it was sent; and the Policy's place on the bus. Also used by
/// [`super::spoken_to`].
pub(super) struct Desk {
    home: crate::process::SessionDirectory,
    bus: std::path::PathBuf,
    decisions: lamp_bus::Publisher,
    pub(super) asked: std::sync::mpsc::Receiver<serde_json::Value>,
}

impl Desk {
    pub(super) fn new(tag: &str) -> Self {
        let home = crate::process::SessionDirectory::create().unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(home.path.join(crate::tools::CALL_SOCKET))
                .unwrap();
        let (told, asked) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                if std::io::BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .is_err()
                {
                    continue;
                }
                // Told before it answers, so a test that has the answer
                // finds what was asked already there.
                if told
                    .send(serde_json::from_str::<serde_json::Value>(&line).unwrap())
                    .is_err()
                {
                    return;
                }
                let _ = stream.write_all(b"{\"ok\":true,\"result\":{\"volume\":60}}\n");
            }
        });
        // Short: a socket's whole path must fit in 103 bytes.
        let bus = std::path::PathBuf::from(format!("/tmp/lph-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bus);
        std::fs::create_dir_all(&bus).unwrap();
        let decisions = lamp_bus::Bus::new(&bus)
            .unwrap()
            .publisher_declared(
                lamp_msgs::Decision::TOPIC,
                &lamp_msgs::Decision::declaration(),
                "policy",
            )
            .unwrap();
        Self {
            home,
            bus,
            decisions,
            asked,
        }
    }

    pub(super) fn options(&self, holds: bool) -> Options {
        Options {
            reporting: Reporting::Typed,
            tools: crate::tools::Settings {
                decline: false,
                exec: Some(self.home.path.clone()),
                memory: None,
                clock: false,
                schedule_tools: true,
                ..Default::default()
            },
            attention: None,
            name_signal: false,
            sounds_checked: false,
            ask_unclear: false,
            name_in_passing_ignored: false,
            policy_holds: holds.then(|| self.bus.clone()),
            wait_for_room: false,
            staff_note: false,
            spoken_to: false,
            voices: None,
        }
    }

    /// The Policy decides about what was said in turn `request`, for real.
    pub(super) fn decide(&mut self, request: Option<u64>, choice: &str) {
        self.decide_with(request, choice, json!({}));
    }

    /// The same, with the model's answers.
    pub(super) fn decide_with(
        &mut self,
        request: Option<u64>,
        choice: &str,
        answers: serde_json::Value,
    ) {
        let decision = json!({"about": "attention", "choice": choice, "probability": 0.235,
                              "answers": answers, "by": "typesafe/jev-1.13-20260917",
                              "took_ms": 540, "shadow": false, "request": request});
        self.decisions.publish_json(0, &decision).unwrap();
    }
}

impl Drop for Desk {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.bus);
    }
}

fn volume_up(id: &str) -> serde_json::Value {
    json!({"toolCall":{"functionCalls":[{"id":id,"name":"volume_up","args":{}}]}})
}

/// The turn the lamp heard, ended; the Policy's decision about it, if any,
/// taken on a tick; then the model asks for `volume_up`. What the model is
/// told, and how long after the call.
async fn asked_for_volume(
    desk: &mut Desk,
    holds: bool,
    choice: Option<&str>,
) -> (LiveRig, Dial, u64, serde_json::Value, Duration) {
    asked_for_volume_about(desk, holds, choice, Some).await
}

/// As [`asked_for_volume`], with the decision about the turn `about` says.
async fn asked_for_volume_about(
    desk: &mut Desk,
    holds: bool,
    choice: Option<&str>,
    about: impl Fn(u64) -> Option<u64>,
) -> (LiveRig, Dial, u64, serde_json::Value, Duration) {
    let mut rig = LiveRig::start_with(quick_policy(), desk.options(holds));
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    if let Some(choice) = choice {
        desk.decide(about(request), choice);
    }
    let called = crate::scenario_clock::instant();
    dial.service.send(volume_up("v1")).await;
    let told = rig.service_message(&mut dial).await;
    (
        rig,
        dial,
        request,
        told,
        crate::scenario_clock::instant() - called,
    )
}

/// The worker's outputs for `long`, its heartbeat kept. In this rig a turn
/// that made a call is not completed, so nothing out is waited for this way.
async fn outputs_for(rig: &mut LiveRig, long: Duration) -> Vec<ProviderOutput> {
    let until = crate::scenario_clock::instant() + long;
    let mut out = Vec::new();
    while crate::scenario_clock::instant() < until {
        rig.publish();
        while let Some(wire) = rig.take() {
            if let Wire::Output(output) = wire {
                out.push(output);
            }
        }
        rig.tick(Duration::from_millis(2)).await;
    }
    out
}

fn response(told: &serde_json::Value) -> serde_json::Value {
    told["toolResponse"]["functionResponses"][0]["response"].clone()
}

/// lamp-4ace, 2026-10-15 14:49: "Thailand" heard in a busy office, the
/// Policy `stay_quiet`, and the lamp turned itself up for eleven hours.
/// Held: the model is told it was not done and to say nothing, the
/// executive is never asked, and what the model says next is not let out.
#[tokio::test(start_paused = true)]
async fn an_action_in_a_turn_the_policy_judged_not_for_the_lamp_is_not_done_and_nothing_is_said() {
    let _clock = logical_clock();
    let mut desk = Desk::new("held");
    let (mut rig, mut dial, request, told, _) =
        asked_for_volume(&mut desk, true, Some("stay_quiet")).await;
    assert_eq!(
        response(&told),
        json!({"done": false, "next": crate::policy_holds::HELD})
    );
    // What the model says next is not let out. (Let out, as when the
    // Policy took the turn as the lamp's below, it comes at once.)
    dial.service.send(provider_audio(9, 480)).await;
    let out = outputs_for(&mut rig, Duration::from_millis(500)).await;
    assert!(
        !out.iter().any(
            |output| matches!(output, ProviderOutput::Audio { request: of, .. } if *of == request)
        ),
        "{out:?}"
    );
    // The executive was never asked. (Never a blocking wait here: the
    // worker shares this test's runtime.)
    assert!(desk.asked.try_recv().is_err());
    rig.stop().await;
}

/// A turn the Policy took as the lamp's (`answer`, or `look_up`): the
/// action runs, and its answer goes back as it would without the switch.
#[tokio::test(start_paused = true)]
async fn an_action_in_a_turn_the_policy_took_as_the_lamps_runs() {
    let _clock = logical_clock();
    for choice in ["answer", "look_up"] {
        let mut desk = Desk::new(choice);
        let (mut rig, mut dial, request, told, _) =
            asked_for_volume(&mut desk, true, Some(choice)).await;
        assert_eq!(response(&told), json!({"volume": 60}), "{choice}");
        assert_eq!(desk.asked.try_recv().unwrap()["name"], "volume_up");
        // And what the model says next is let out.
        dial.service.send(provider_audio(9, 480)).await;
        let out = outputs_for(&mut rig, Duration::from_millis(500)).await;
        assert!(
            out.iter().any(
                |output| matches!(output, ProviderOutput::Audio { request: of, .. } if *of == request)
            ),
            "{choice}: {out:?}"
        );
        rig.stop().await;
    }
}

/// No decision, or only a shadow one: after the wait the action runs as it
/// did; without the switch it runs at once, whatever was decided.
#[tokio::test(start_paused = true)]
async fn with_no_decision_in_time_it_runs_after_the_wait_and_without_the_switch_at_once() {
    let _clock = logical_clock();
    let mut desk = Desk::new("late");
    let (mut rig, _dial, _, told, took) = asked_for_volume(&mut desk, true, None).await;
    assert_eq!(response(&told), json!({"volume": 60}));
    assert!(
        took >= crate::policy_holds::WAIT - Duration::from_millis(100),
        "{took:?}"
    );
    rig.stop().await;

    let mut desk = Desk::new("off");
    let (mut rig, _dial, _, told, took) =
        asked_for_volume(&mut desk, false, Some("stay_quiet")).await;
    assert_eq!(response(&told), json!({"volume": 60}));
    assert!(took < Duration::from_secs(1), "{took:?}");
    rig.stop().await;
}

/// The reviewer on #908, 05:09: a decision about another turn, the one
/// before or one that does not say, holds nothing here. With none about
/// its own turn the action runs after the wait, as without a decision.
#[tokio::test(start_paused = true)]
async fn another_turns_stay_quiet_holds_nothing() {
    let _clock = logical_clock();
    for (tag, about) in [
        (
            "before",
            (|request: u64| Some(request - 1)) as fn(u64) -> Option<u64>,
        ),
        ("nobody", |_| None),
    ] {
        let mut desk = Desk::new(tag);
        let (mut rig, _dial, _, told, took) =
            asked_for_volume_about(&mut desk, true, Some("stay_quiet"), about).await;
        assert_eq!(response(&told), json!({"volume": 60}), "{tag}");
        assert!(
            took >= crate::policy_holds::WAIT - Duration::from_millis(100),
            "{tag}: {took:?}"
        );
        rig.stop().await;
    }
}

/// The worker reads `--policy-holds-actions` where the coordinator puts it,
/// after `--sounds-checked`.
#[test]
fn the_switch_is_read_after_sounds_checked() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::not_speech::CHECKED_FLAG, crate::policy_holds::FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.sounds_checked && files.policy_holds);
    let (_, _, files, _) = parse_flags(dir, &[]).unwrap();
    assert!(!files.policy_holds);
    // Out of its place it is refused.
    let wrong: Vec<String> = [crate::policy_holds::FLAG, crate::not_speech::CHECKED_FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// The turn heard, ended, and the model asking for `volume_up`, kept for
/// the Policy's decision, which never comes here.
async fn parked_volume(desk: &mut Desk) -> (LiveRig, Dial) {
    parked_volume_on(LiveRig::start_with(quick_policy(), desk.options(true))).await
}

async fn parked_volume_on(mut rig: LiveRig) -> (LiveRig, Dial) {
    let mut dial = rig.ready().await;
    rig.speak(&mut dial, false, true).await;
    dial.service.send(volume_up("v1")).await;
    // Kept: nothing is answered at once.
    assert!(
        outputs_for(&mut rig, Duration::from_millis(100))
            .await
            .is_empty()
    );
    (rig, dial)
}

/// The reviewer on #908: a call kept for the Policy's decision whose turn
/// is cancelled meanwhile (its owner gone) is never run, also after the
/// wait in which an uncancelled one would have run, the worker healthy all
/// through and stopped cleanly at the end (the reviewer, 05:52).
#[tokio::test(start_paused = true)]
async fn a_kept_action_whose_turn_is_cancelled_is_never_run() {
    let _clock = logical_clock();
    let mut desk = Desk::new("cancelled");
    let (mut rig, mut dial) = parked_volume(&mut desk).await;
    let owner = rig.snapshot.owner().unwrap();
    rig.owner.cancel_turn(real_time(), owner).unwrap();
    rig.publish();
    // The stop as the session sends it, an interruption of the answer that
    // waits on the call; settled only once it has come, as Gemini settles
    // one. A stopped turn the service never settles would end the worker at
    // its barrier's bound.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let message = rig.service_message(&mut dial).await;
        if message["realtimeInput"].get("activityStart").is_some() {
            break;
        }
        // Only the retired call's answer may come before it.
        assert_eq!(response(&message), json!({"done": false}), "{message}");
        assert!(Instant::now() < deadline, "the stop never came");
    }
    dial.service
        .send(json!({"serverContent":{"interrupted":true}}))
        .await;
    dial.service.send(idle_complete()).await;
    // Never run, through the wait in which an uncancelled one would have
    // run, the worker alive and taking its heartbeat all along.
    let until =
        crate::scenario_clock::instant() + crate::policy_holds::WAIT + Duration::from_millis(300);
    while crate::scenario_clock::instant() < until {
        assert!(!rig.worker.is_finished(), "the worker ended");
        rig.publish();
        while rig.take().is_some() {}
        rig.tick(Duration::from_millis(5)).await;
    }
    assert!(desk.asked.try_recv().is_err(), "volume_up was run");
    // And it stops cleanly: its result is Ok.
    rig.stop().await;
}

/// The reviewer on #908: a call the service withdrew while it was kept is
/// never run.
#[tokio::test(start_paused = true)]
async fn a_kept_action_the_service_withdrew_is_never_run() {
    let _clock = logical_clock();
    let mut desk = Desk::new("withdrawn");
    let (mut rig, mut dial) = parked_volume(&mut desk).await;
    dial.service
        .send(json!({"toolCallCancellation":{"ids":["v1"]}}))
        .await;
    outputs_for(
        &mut rig,
        crate::policy_holds::WAIT + Duration::from_millis(300),
    )
    .await;
    assert!(desk.asked.try_recv().is_err(), "volume_up was run");
    rig.stop().await;
}

/// The reviewer on #908, 05:11: with the output full (the coordinator
/// takes nothing, so no credit comes back), the worker takes no provider
/// event, and the service's cancellation waits behind the sound before it.
/// The call is still never run: the session is asked right before.
#[tokio::test(start_paused = true)]
async fn a_kept_action_withdrawn_while_the_output_is_full_is_never_run() {
    let _clock = logical_clock();
    let desk = Desk::new("full");
    let rig = LiveRig::start_capacity(
        quick_policy(),
        desk.options(true),
        BARRIER,
        false,
        Continuation::Off,
        lamp_gemini::MAX_WIRE_BYTES * 16,
    );
    let (mut rig, mut dial) = parked_volume_on(rig).await;
    // Twenty seconds of sound at 24 kHz, 500 packets: more than the worker
    // keeps before it stops taking events. Then the cancellation. The
    // heartbeat is kept between the sends: without it the turn's leases run
    // out and its owner is cancelled, which also lets the call go.
    for value in 1..=20 {
        dial.service.send(provider_audio(value, 24_000)).await;
        rig.publish();
    }
    dial.service
        .send(json!({"toolCallCancellation":{"ids":["v1"]}}))
        .await;
    rig.publish();
    // Past the wait in which an uncancelled call would have run, nothing
    // taken from the worker, its heartbeat kept.
    rig.pump(crate::policy_holds::WAIT + Duration::from_millis(300))
        .await;
    assert!(desk.asked.try_recv().is_err(), "volume_up was run");
    rig.stop().await;
}

/// The first builder on #908 (05:57): listing reminders is not held. On
/// `stay_quiet` without the name it is asked at once, as without the
/// switch.
#[tokio::test(start_paused = true)]
async fn listing_reminders_is_not_held() {
    let _clock = logical_clock();
    let mut desk = Desk::new("list");
    let mut rig = LiveRig::start_with(quick_policy(), desk.options(true));
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    desk.decide(Some(request), "stay_quiet");
    let called = crate::scenario_clock::instant();
    dial.service
        .send(json!({"toolCall":{"functionCalls":[
            {"id":"l1","name":"list_reminders","args":{}}
        ]}}))
        .await;
    let told = rig.service_message(&mut dial).await;
    assert_ne!(
        response(&told),
        json!({"done": false, "next": crate::policy_holds::HELD})
    );
    assert!(crate::scenario_clock::instant() - called < Duration::from_secs(1));
    // The schedule was asked (through the same socket here).
    assert!(desk.asked.try_recv().is_ok());
    rig.stop().await;
}

/// The first builder on #908 (05:02): "Yes, give me five minutes", said to
/// someone else in a conversation, set a five-minute timer. A timer is held
/// as the volume is: on `stay_quiet` it is not set, and the executive is
/// never asked.
#[tokio::test(start_paused = true)]
async fn a_timer_in_a_turn_the_policy_judged_not_for_the_lamp_is_not_set() {
    let _clock = logical_clock();
    let mut desk = Desk::new("timer");
    let mut rig = LiveRig::start_with(quick_policy(), desk.options(true));
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    desk.decide(Some(request), "stay_quiet");
    dial.service
        .send(json!({"toolCall":{"functionCalls":[
            {"id":"t1","name":"set_timer","args":{"minutes":5}}
        ]}}))
        .await;
    let told = rig.service_message(&mut dial).await;
    assert_eq!(
        response(&told),
        json!({"done": false, "next": crate::policy_holds::HELD})
    );
    assert!(desk.asked.try_recv().is_err(), "set_timer was run");
    rig.stop().await;
}

/// #1054, the quiet-room case #1090 leaves (the first builder's 15:39 on
/// #829): "Yes, give me five minutes." after someone else's "Are you coming
/// to the meeting at two?", which the lamp left unanswered, follows that
/// line, not the lamp. Though the Policy's `follows_lamp` is 0.72, the timer
/// is held, as in any `stay_quiet`.
#[tokio::test]
async fn a_timer_in_a_turn_after_a_line_the_lamp_left_unanswered_is_held() {
    let mut desk = Desk::new("after-other");
    // With the voice's own rule for whom it answers (`--attention`), which
    // follows each line: in a quiet room, nobody near.
    let options = Options {
        attention: Some(|| (crate::room_told::Room::Quiet, false)),
        ..desk.options(true)
    };
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let first = rig.speak(&mut dial, false, true).await;
    dial.service
        .send(json!({"serverContent":{"inputTranscription":{"text":"Are you coming to the meeting at two?"}}}))
        .await;
    dial.service.send(idle_complete()).await;
    loop {
        match rig.output().await {
            ProviderOutput::Audio { request, .. } if request == first => {
                panic!("the line was answered")
            }
            ProviderOutput::TurnComplete { request, .. } if request == first => break,
            _ => {}
        }
    }
    rig.owner
        .complete_turn(real_time(), rig.snapshot.owner().unwrap())
        .unwrap();
    rig.publish();
    let request = rig.speak(&mut dial, false, true).await;
    dial.service
        .send(json!({"serverContent":{"inputTranscription":{"text":"Yes, give me five minutes."}}}))
        .await;
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
    assert_eq!(
        response(&told),
        json!({"done": false, "next": crate::policy_holds::HELD})
    );
    assert!(desk.asked.try_recv().is_err(), "set_timer was run");
    rig.stop().await;
}

/// Someone says something and the lamp answers it aloud, and the turn ends:
/// the lamp has just spoken.
async fn the_lamp_answers(rig: &mut LiveRig, dial: &mut Dial) {
    let first = rig.speak(dial, false, true).await;
    dial.service.send(provider_audio(9, 480)).await;
    dial.service.send(idle_complete()).await;
    loop {
        if let ProviderOutput::TurnComplete { request, .. } = rig.output().await
            && request == first
        {
            break;
        }
    }
    rig.owner
        .complete_turn(real_time(), rig.snapshot.owner().unwrap())
        .unwrap();
    rig.publish();
}

/// #1040: in conversation with the lamp, a `stay_quiet` whose
/// `follows_lamp` is 0.6 or more does not hold what the schedule sets: the
/// timer is set. (The 11:20 turn on lamp-4ace that #1040 began with scored
/// 0.35, and stays held at 0.6: the reviewer on #1054.) Without a line of
/// the lamp's lately, the same turn follows nothing of the lamp's and the
/// timer is held (the reviewer on #1054). The volume in such a turn is
/// still held, and so is a cancel, which is silent and cannot be undone
/// (Fourth on #1054).
#[tokio::test]
async fn a_timer_in_a_turn_that_follows_the_lamp_is_set_though_the_policy_said_stay_quiet() {
    for spoke in [true, false] {
        let mut desk = Desk::new(if spoke { "follows" } else { "follows-silent" });
        let mut rig = LiveRig::start_with(quick_policy(), desk.options(true));
        let mut dial = rig.ready().await;
        if spoke {
            the_lamp_answers(&mut rig, &mut dial).await;
        }
        let request = rig.speak(&mut dial, false, true).await;
        desk.decide_with(
            Some(request),
            "stay_quiet",
            json!({"calls_lamp": 0.05, "follows_lamp": 0.72}),
        );
        dial.service
            .send(json!({"toolCall":{"functionCalls":[
                {"id":"t1","name":"set_timer","args":{"minutes":1}}
            ]}}))
            .await;
        let told = rig.service_message(&mut dial).await;
        assert_eq!(
            response(&told) == json!({"done": false, "next": crate::policy_holds::HELD}),
            !spoke,
            "the lamp spoke lately: {spoke}"
        );
        assert_eq!(
            desk.asked.try_recv().is_ok(),
            spoke,
            "set_timer run: {spoke}"
        );
        rig.stop().await;
    }

    let mut desk = Desk::new("follows-volume");
    let mut rig = LiveRig::start_with(quick_policy(), desk.options(true));
    let mut dial = rig.ready().await;
    // In conversation, so that it is held for what it is.
    the_lamp_answers(&mut rig, &mut dial).await;
    let request = rig.speak(&mut dial, false, true).await;
    desk.decide_with(
        Some(request),
        "stay_quiet",
        json!({"calls_lamp": 0.05, "follows_lamp": 0.72}),
    );
    dial.service.send(volume_up("v1")).await;
    let told = rig.service_message(&mut dial).await;
    assert_eq!(
        response(&told),
        json!({"done": false, "next": crate::policy_holds::HELD})
    );
    assert!(desk.asked.try_recv().is_err(), "volume_up was run");
    rig.stop().await;

    let mut desk = Desk::new("follows-cancel");
    let mut rig = LiveRig::start_with(quick_policy(), desk.options(true));
    let mut dial = rig.ready().await;
    // In conversation, so that it is held for what it is.
    the_lamp_answers(&mut rig, &mut dial).await;
    let request = rig.speak(&mut dial, false, true).await;
    desk.decide_with(
        Some(request),
        "stay_quiet",
        json!({"calls_lamp": 0.05, "follows_lamp": 0.72}),
    );
    dial.service
        .send(json!({"toolCall":{"functionCalls":[
            {"id":"c1","name":"cancel_reminder","args":{"id":1}}
        ]}}))
        .await;
    let told = rig.service_message(&mut dial).await;
    assert_eq!(
        response(&told),
        json!({"done": false, "next": crate::policy_holds::HELD})
    );
    assert!(desk.asked.try_recv().is_err(), "cancel_reminder was run");
    rig.stop().await;
}

/// `--provider-patience` is read where the coordinator puts it, after
/// `--staff-note`, on both routes (#705, 17 Oct).
#[test]
fn provider_patience_is_read_after_the_staff_note() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [crate::staff_note::FLAG, crate::config::PATIENCE_FLAG]
        .map(String::from)
        .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.provider_patience);
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert!(files.provider_patience);
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert!(!files.provider_patience);
    let wrong: Vec<String> = [crate::config::PATIENCE_FLAG, crate::staff_note::FLAG]
        .map(String::from)
        .into();
    assert!(parse_flags(dir, &wrong).is_err());
}

/// The worker reads `--transcription-language` and its tag where the
/// coordinator puts them, after `--ask-unclear` and before
/// `--policy-holds-actions`, on both routes (#926, the reviewer on #1367).
#[test]
fn the_transcription_language_is_read_after_ask_unclear() {
    let dir = std::path::Path::new("/run/test");
    let flags: Vec<String> = [
        crate::not_speech::UNCLEAR_FLAG,
        crate::not_speech::TRANSCRIPTION_FLAG,
        "en-US",
        crate::policy_holds::FLAG,
    ]
    .map(String::from)
    .into();
    let (_, _, files, _) = parse_flags(dir, &flags).unwrap();
    assert!(files.ask_unclear && files.policy_holds);
    assert_eq!(files.transcription_language.as_deref(), Some("en-US"));
    let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
    assert_eq!(files.transcription_language.as_deref(), Some("en-US"));
    let (_, _, files, _) = parse_flags(dir, &flags[..1]).unwrap();
    assert_eq!(files.transcription_language, None);
    // Out of its place it is refused.
    let wrong: Vec<String> = [
        crate::not_speech::TRANSCRIPTION_FLAG,
        "en-US",
        crate::not_speech::UNCLEAR_FLAG,
    ]
    .map(String::from)
    .into();
    assert!(parse_flags(dir, &wrong).is_err());
}
