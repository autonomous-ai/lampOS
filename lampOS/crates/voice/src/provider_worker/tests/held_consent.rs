//! A call that withdraws consent is done though its line's answer is held
//! (the first builder on #1722, 16:44; #1683): through the worker's own
//! loop against the scripted service, with the lamp speaking when spoken
//! to (`LAMP_SPOKEN_TO=on`), a line nobody said to the lamp whose model
//! calls `reports_stop` stops the reports, on a bus of the test's own, and
//! none of its sound is played; a light or volume call on the same held
//! line is still answered and not run; and a line said to the lamp stops
//! them and is answered aloud, as before. A held stop is kept by
//! `lamp-cloud` as held, the room is not told the line was answered, the
//! journal says it was done, and nothing of it is said unasked; asked
//! about reports, the model is told who stopped them and when (Third's
//! read of #1880, the first builder on #1722, 18:44).
use super::policy_holds::Desk;
use super::*;
use crate::provider_worker::seen_in_tests;
use lamp_msgs::{ReportsBy, ReportsRequest, ReportsState};

/// The tests here share [`crate::reports_stop::TEST_ROOT`]: one at a time.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn quiet() -> (crate::room_told::Room, bool) {
    (crate::room_told::Room::Quiet, false)
}

fn heard(text: &str) -> serde_json::Value {
    json!({"serverContent":{"inputTranscription":{"text":text}}})
}

/// A colleague's line, said to someone else: held with the switch.
const NOT_TO_THE_LAMP: &str = "Did you get my message about the budget?";
const TO_THE_LAMP: &str = "Hey Lamp, stop sending reports.";

fn wall_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Reports on, as `lamp-cloud` first says them.
fn sending() -> ReportsState {
    ReportsState {
        share: true,
        queued: 1,
        ..Default::default()
    }
}

/// Keeps the stand-in `lamp-cloud` listening until dropped.
struct Listening(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Drop for Listening {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// `lamp-cloud` on the test's bus: `first` said, and off once asked, kept
/// as the voice's, held as asked, and when. Each ask it takes is sent on
/// the channel. It listens until the [`Listening`] is dropped: with the
/// whole crate's tests running, the rig took over 5 s to reach its call,
/// and a stand-in that left after 5 s of nothing had its ask reach no one
/// (#1569 row 66). Like `lamp-cloud`, it says its state again while it
/// listens: said once, `reports/state` means nothing after its 15 s
/// (`ReportsState::declaration`), and a rig that slow read `before` as no
/// state and answered `NOT_SENDING` (the held_consent flake, 29 Oct).
fn cloud(
    bus: &std::path::Path,
    first: ReportsState,
) -> (std::sync::mpsc::Receiver<ReportsRequest>, Listening) {
    let bus = lamp_bus::Bus::new(bus).unwrap();
    let mut taking = bus
        .subscribe_typed(ReportsRequest::TOPIC, "cloud", ReportsRequest::TYPE)
        .unwrap();
    let mut state = bus
        .publisher_declared(ReportsState::TOPIC, &ReportsState::declaration(), "cloud")
        .unwrap();
    state
        .publish_json(lamp_ipc::monotonic_us(), &first)
        .unwrap();
    let (told, asks) = std::sync::mpsc::channel();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let listening = Listening(done.clone());
    std::thread::spawn(move || {
        let mut current = first;
        let mut said = std::time::Instant::now();
        while !done.load(std::sync::atomic::Ordering::Relaxed) {
            if said.elapsed() >= Duration::from_secs(1) {
                let _ = state.publish_json(lamp_ipc::monotonic_us(), &current);
                said = std::time::Instant::now();
            }
            let Ok(taken) = taking.recv(Duration::from_millis(100)) else {
                return;
            };
            let Some(message) = taken else {
                continue;
            };
            let ask = message.parse::<ReportsRequest>().unwrap();
            current = ReportsState {
                share: ask.share,
                queued: 0,
                by: Some(ReportsBy::Voice),
                held: ask.held,
                at_s: Some(wall_s()),
            };
            let _ = state.publish_json(lamp_ipc::monotonic_us(), &current);
            said = std::time::Instant::now();
            if told.send(ask).is_err() {
                return;
            }
        }
    });
    (asks, listening)
}

/// What came of a line heard as `text` whose answer calls `function`: what
/// the service was answered, the asks `lamp-cloud` took, what the
/// executive was asked, whether any of the line's sound was played, the
/// line's request and its call's id, and what else the service was sent
/// after the call's answer.
struct Came {
    answer: serde_json::Value,
    asks: Vec<ReportsRequest>,
    executive: Vec<serde_json::Value>,
    played: bool,
    request: u64,
    id: String,
    later: Vec<serde_json::Value>,
}

async fn line(tag: &str, text: &str, function: &str, allowlisted: bool) -> Came {
    line_with(tag, text, function, allowlisted, sending()).await
}

async fn line_with(
    tag: &str,
    text: &str,
    function: &str,
    allowlisted: bool,
    first: ReportsState,
) -> Came {
    let desk = Desk::new(tag);
    *crate::reports_stop::TEST_ROOT.lock().unwrap() = Some(desk.bus());
    let (asks, _listening) = cloud(&desk.bus(), first);
    // An id of the test's own: the record of what the room was told is
    // shared by every test in the binary.
    let id = format!("{tag}-c9");
    let mut options = desk.options(false);
    options.attention = Some(quiet);
    options.spoken_to = true;
    options.tools.reports_stop = allowlisted;
    let mut rig = LiveRig::start_with(quick_policy(), options);
    let mut dial = rig.ready().await;
    let request = rig.speak(&mut dial, false, true).await;
    dial.service.send(heard(text)).await;
    assert!(matches!(
        rig.output().await,
        ProviderOutput::Transcript { request: None, .. }
    ));
    dial.service.send(function_call(&id, function)).await;
    let answer = rig.service_message(&mut dial).await;
    // What the model says after its call's answer ("I've stopped sending
    // reports."), watched for half a second with the worker's lease kept
    // fresh: in this rig a turn that made a call is not completed
    // (`policy_holds`' tests).
    dial.service.send(provider_audio(7, 240)).await;
    let mut played = false;
    let mut later = Vec::new();
    let until = crate::scenario_clock::instant() + Duration::from_millis(500);
    while crate::scenario_clock::instant() < until {
        rig.publish();
        while let Some(wire) = rig.take() {
            played |= matches!(wire, Wire::Output(ProviderOutput::Audio { request: of, .. }) if of == request);
        }
        if let Some(message) = dial.service.try_receive(Duration::from_millis(1)).await {
            later.push(message);
        }
        rig.tick(Duration::from_millis(2)).await;
    }
    rig.stop().await;
    *crate::reports_stop::TEST_ROOT.lock().unwrap() = None;
    Came {
        answer,
        asks: asks.try_iter().collect(),
        executive: desk.asked.try_iter().collect(),
        played,
        request,
        id,
        later,
    }
}

impl Came {
    fn response(&self, function: &str, told: serde_json::Value) -> serde_json::Value {
        json!({"toolResponse":{"functionResponses":[
            {"id":self.id,"name":function,"response":told}
        ]}})
    }

    /// Whether anything sent to the service after the call's answer speaks
    /// of reports: a note, a turn, anything the lamp would say unasked.
    fn told_of_reports_later(&self) -> bool {
        self.later
            .iter()
            .any(|message| message.to_string().to_lowercase().contains("report"))
    }
}

/// "Lamp, stop sending reports." held as not the lamp's (#1796's spoken
/// stop on lamp-06c1): the stop is asked of `lamp-cloud` on the bus as
/// `{"share": false}` and the model is told it is done, while the answer
/// stays held: none of its sound is played.
#[tokio::test]
async fn a_held_lines_reports_stop_is_done_and_its_answer_stays_held() {
    let _one = ONE_AT_A_TIME.lock().await;
    let came = line("hcs1", NOT_TO_THE_LAMP, crate::reports_stop::NAME, true).await;
    assert_eq!(
        came.answer,
        came.response(
            crate::reports_stop::NAME,
            json!({"ok": true, "say": crate::reports_stop::STOPPED})
        )
    );
    // Asked as held, for `lamp-cloud` to keep it so (#1880).
    assert_eq!(
        came.asks,
        [ReportsRequest {
            share: false,
            held: true
        }]
    );
    assert!(!came.played, "the held answer was played");
    assert!(came.executive.is_empty());
    // The room is not told the line was answered: the held run is not the
    // lamp answering (Third's read of #1880).
    assert!(
        !seen_in_tests::was_told_answered(&came.id),
        "the room was told a held line was answered"
    );
    // The journal says it was done, and only that.
    assert_eq!(
        seen_in_tests::held_done_lines(&came.id),
        [format!(
            "request {}: reports_stop done though the answer was held (consent withdrawn)",
            came.request
        )]
    );
    // Nothing of it is said unasked: nothing about reports goes to the
    // service after the call's answer.
    assert!(!came.told_of_reports_later(), "{:?}", came.later);
}

/// Every other call on a held line stays held as before: a light or a
/// volume call is answered and not run, and nothing is asked on the bus.
/// The voice has no call that turns reports on, and a name like one is
/// not on the list either.
#[tokio::test]
async fn a_held_lines_other_calls_are_still_not_done() {
    let _one = ONE_AT_A_TIME.lock().await;
    for (tag, function) in [("hcs2", "light_on"), ("hcs3", "volume_up")] {
        let came = line(tag, NOT_TO_THE_LAMP, function, true).await;
        assert_eq!(
            came.answer,
            came.response(function, json!({"ok": true})),
            "{function}"
        );
        assert!(came.executive.is_empty(), "{function} was run");
        assert!(came.asks.is_empty(), "{function}");
        assert!(!came.played, "{function}");
    }
    assert_eq!(
        crate::attention::DONE_THOUGH_HELD,
        [crate::reports_stop::NAME]
    );
    for function in [
        "reports_start",
        "light_on",
        "volume_up",
        "remember",
        "use_skill",
    ] {
        assert!(!crate::attention::done_though_held(function), "{function}");
    }
}

/// Said to the lamp, the stop is done and answered aloud, as before; and
/// without `LAMP_REPORTS=on`, a held line's `reports_stop` is not the
/// voice's to run at all.
#[tokio::test]
async fn an_unheld_reports_stop_is_done_and_answered_as_before() {
    let _one = ONE_AT_A_TIME.lock().await;
    let came = line("hcs4", TO_THE_LAMP, crate::reports_stop::NAME, true).await;
    assert_eq!(
        came.answer,
        came.response(
            crate::reports_stop::NAME,
            json!({"ok": true, "say": crate::reports_stop::STOPPED})
        )
    );
    assert_eq!(came.asks, [ReportsRequest::default()]);
    assert!(came.played, "the answer said to the lamp was not played");
    // Done and heard: the room is told it was answered, and the journal
    // has no held line for it.
    assert!(seen_in_tests::was_told_answered(&came.id));
    assert!(seen_in_tests::held_done_lines(&came.id).is_empty());
    let came = line("hcs5", NOT_TO_THE_LAMP, crate::reports_stop::NAME, false).await;
    assert!(came.asks.is_empty());
    assert!(!came.played);
}

/// The first builder on #1722, 18:44: asked about reports after a held
/// stop, the model is told that someone asked the lamp to stop them, when,
/// and that they are turned back on in the app; and its answer, said to
/// the lamp, is played.
#[tokio::test]
async fn asked_about_reports_after_a_held_stop_it_says_who_and_when() {
    let _one = ONE_AT_A_TIME.lock().await;
    let stopped = ReportsState {
        share: false,
        queued: 1,
        by: Some(ReportsBy::Voice),
        held: true,
        at_s: Some(wall_s() - 120),
    };
    let came = line_with(
        "hcs6",
        "Hey Lamp, are you sending reports?",
        crate::reports_stop::STATE_NAME,
        true,
        stopped,
    )
    .await;
    let told = &came.answer["toolResponse"]["functionResponses"][0]["response"];
    assert_eq!(told["sending"], false, "{told}");
    let say = told["say"].as_str().unwrap();
    assert!(say.contains("Someone asked me to stop them at "), "{say}");
    assert!(say.contains("today") || say.contains("yesterday"), "{say}");
    assert!(
        say.ends_with("They can be turned back on in the app."),
        "{say}"
    );
    // A lookup: nothing asked of `lamp-cloud`, nothing done.
    assert!(came.asks.is_empty());
    assert!(came.played);
    assert!(!seen_in_tests::was_told_answered(&came.id));
}
