//! A call that withdraws consent is done though its line's answer is held
//! (the first builder on #1722, 16:44; #1683): through the worker's own
//! loop against the scripted service, with the lamp speaking when spoken
//! to (`LAMP_SPOKEN_TO=on`), a line nobody said to the lamp whose model
//! calls `reports_stop` stops the reports, on a bus of the test's own, and
//! none of its sound is played; a light or volume call on the same held
//! line is still answered and not run; and a line said to the lamp stops
//! them and is answered aloud, as before.
use super::policy_holds::Desk;
use super::*;
use lamp_msgs::{ReportsRequest, ReportsState};

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

/// `lamp-cloud` on the test's bus: reports on, and off once asked. Each
/// ask it takes is sent on the channel.
fn cloud(bus: &std::path::Path) -> std::sync::mpsc::Receiver<ReportsRequest> {
    let bus = lamp_bus::Bus::new(bus).unwrap();
    let mut taking = bus
        .subscribe_typed(ReportsRequest::TOPIC, "cloud", ReportsRequest::TYPE)
        .unwrap();
    let mut state = bus
        .publisher_declared(ReportsState::TOPIC, &ReportsState::declaration(), "cloud")
        .unwrap();
    state
        .publish_json(
            lamp_ipc::monotonic_us(),
            &ReportsState {
                share: true,
                queued: 1,
            },
        )
        .unwrap();
    let (told, asks) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(Some(message)) = taking.recv(Duration::from_secs(5)) {
            let ask = message.parse::<ReportsRequest>().unwrap();
            let _ = state.publish_json(
                lamp_ipc::monotonic_us(),
                &ReportsState {
                    share: ask.share,
                    queued: 0,
                },
            );
            if told.send(ask).is_err() {
                return;
            }
        }
    });
    asks
}

/// What came of a line heard as `text` whose answer calls `function`: what
/// the service was answered, the asks `lamp-cloud` took, what the
/// executive was asked, and whether any of the line's sound was played.
struct Came {
    answer: serde_json::Value,
    asks: Vec<ReportsRequest>,
    executive: Vec<serde_json::Value>,
    played: bool,
}

async fn line(tag: &str, text: &str, function: &str, allowlisted: bool) -> Came {
    let desk = Desk::new(tag);
    *crate::reports_stop::TEST_ROOT.lock().unwrap() = Some(desk.bus());
    let asks = cloud(&desk.bus());
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
    dial.service.send(function_call("c9", function)).await;
    let answer = rig.service_message(&mut dial).await;
    // What the model says after its call's answer ("I've stopped sending
    // reports."), watched for half a second with the worker's lease kept
    // fresh: in this rig a turn that made a call is not completed
    // (`policy_holds`' tests).
    dial.service.send(provider_audio(7, 240)).await;
    let mut played = false;
    let until = crate::scenario_clock::instant() + Duration::from_millis(500);
    while crate::scenario_clock::instant() < until {
        rig.publish();
        while let Some(wire) = rig.take() {
            played |= matches!(wire, Wire::Output(ProviderOutput::Audio { request: of, .. }) if of == request);
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
    }
}

fn response(function: &str, told: serde_json::Value) -> serde_json::Value {
    json!({"toolResponse":{"functionResponses":[
        {"id":"c9","name":function,"response":told}
    ]}})
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
        response(
            crate::reports_stop::NAME,
            json!({"ok": true, "say": crate::reports_stop::STOPPED})
        )
    );
    assert_eq!(came.asks, [ReportsRequest { share: false }]);
    assert!(!came.played, "the held answer was played");
    assert!(came.executive.is_empty());
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
            response(function, json!({"ok": true})),
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
        response(
            crate::reports_stop::NAME,
            json!({"ok": true, "say": crate::reports_stop::STOPPED})
        )
    );
    assert_eq!(came.asks, [ReportsRequest { share: false }]);
    assert!(came.played, "the answer said to the lamp was not played");
    let came = line("hcs5", NOT_TO_THE_LAMP, crate::reports_stop::NAME, false).await;
    assert!(came.asks.is_empty());
    assert!(!came.played);
}
