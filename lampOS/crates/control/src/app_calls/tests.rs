use super::*;
use std::time::Duration;

fn frame(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

/// A running app with one call in flight, sent at `t0`.
fn in_flight(t0: Instant) -> Live {
    let mut live = Live::default();
    live.connection("gym-coach", true);
    live.begin_call("gym-coach", "app-1-2-3", t0).unwrap();
    live
}

#[test]
fn an_answer_is_taken_once_for_its_own_call_and_told_with_its_result() {
    let t0 = Instant::now();
    let mut live = in_flight(t0);
    let answer = |call: &str| {
        frame(json!({"id": 4, "op": "app.answer", "call": call, "result": {"sets": 3}}))
    };
    // Another call's id, and another app's answer, are not taken.
    let wrong = answer_op(&mut live, "gym-coach", &answer("app-1-2-9"), 4, true, t0);
    assert_eq!(wrong, json!({"re": 4, "ok": true, "taken": false}));
    let foreign = answer_op(&mut live, "other-app", &answer("app-1-2-3"), 4, true, t0);
    assert_eq!(foreign["taken"], false);
    assert!(live.drain_called().is_empty());
    let ok = answer_op(&mut live, "gym-coach", &answer("app-1-2-3"), 4, true, t0);
    assert_eq!(ok, json!({"re": 4, "ok": true, "taken": true}));
    let second = answer_op(&mut live, "gym-coach", &answer("app-1-2-3"), 5, true, t0);
    assert_eq!(second["taken"], false, "once");
    let told = live.drain_called();
    assert_eq!(
        told,
        vec![AppCalled {
            call: "app-1-2-3".into(),
            app: "gym-coach".into(),
            result: Some(json!({"sets": 3})),
            error: None,
        }]
    );
    assert!(told[0].check().is_ok());
}

#[test]
fn an_answer_is_one_result_object_or_one_line_of_the_apps_own_words() {
    let t0 = Instant::now();
    let refused = |value: Value| {
        let mut live = in_flight(t0);
        let reply = answer_op(&mut live, "gym-coach", &frame(value), 4, true, t0);
        assert!(live.drain_called().is_empty(), "nothing told");
        reply["error"]["code"].as_str().map(str::to_owned)
    };
    let call = "app-1-2-3";
    for bad in [
        json!({"id": 4, "op": "app.answer", "call": call}),
        json!({"id": 4, "op": "app.answer", "call": call, "result": {}, "error": "no"}),
        json!({"id": 4, "op": "app.answer", "call": call, "result": [1]}),
        json!({"id": 4, "op": "app.answer", "call": call, "error": 3}),
        json!({"id": 4, "op": "app.answer", "call": call, "error": ""}),
        json!({"id": 4, "op": "app.answer", "call": call, "error": "two\nlines"}),
        json!({"id": 4, "op": "app.answer", "result": {}}),
        json!({"id": 4, "op": "app.answer", "call": call, "result": {}, "more": 1}),
    ] {
        assert_eq!(
            refused(bad.clone()).as_deref(),
            Some("bad_request"),
            "{bad}"
        );
    }
    // The lamp's own words are the lamp's: an app can't say them.
    for word in AppCalled::LAMP_WORDS {
        let bad = json!({"id": 4, "op": "app.answer", "call": call, "error": word});
        assert_eq!(refused(bad).as_deref(), Some("bad_request"), "{word}");
    }
    // Off: refused, nothing taken.
    let mut live = in_flight(t0);
    let off = answer_op(
        &mut live,
        "gym-coach",
        &frame(json!({"id": 4, "op": "app.answer", "call": call, "result": {}})),
        4,
        false,
        t0,
    );
    assert_eq!(off["error"]["code"], "off");
    assert!(live.drain_called().is_empty());
    // The app's own error line is told as it is.
    let mut live = in_flight(t0);
    let own = frame(json!({"id": 4, "op": "app.answer", "call": call, "error": "no plan yet"}));
    assert_eq!(
        answer_op(&mut live, "gym-coach", &own, 4, true, t0)["taken"],
        true
    );
    assert_eq!(live.drain_called()[0].error.as_deref(), Some("no plan yet"));
}

#[test]
fn an_answer_over_its_bound_ends_the_call_as_too_long() {
    let t0 = Instant::now();
    let call = "app-1-2-3";
    let big = "x".repeat(lamp_msgs::ARGS_MOST_BYTES);
    let long = "y".repeat(lamp_msgs::ERROR_MOST + 1);
    for over in [
        json!({"id": 4, "op": "app.answer", "call": call, "result": {"text": big}}),
        json!({"id": 4, "op": "app.answer", "call": call, "error": long}),
    ] {
        let mut live = in_flight(t0);
        assert_eq!(
            answer_op(&mut live, "gym-coach", &frame(over), 4, true, t0)["taken"],
            true
        );
        let told = live.drain_called();
        assert_eq!(told[0].error.as_deref(), Some("too_long"));
        assert!(told[0].check().is_ok());
    }
}

#[test]
fn one_call_an_app_in_flight_for_its_wait_then_timeout_and_a_late_answer_is_not_taken() {
    let t0 = Instant::now();
    let mut live = Live::default();
    assert_eq!(
        live.begin_call("gym-coach", "app-1-2-3", t0),
        Err("not running")
    );
    live.connection("gym-coach", true);
    live.begin_call("gym-coach", "app-1-2-3", t0).unwrap();
    assert_eq!(live.begin_call("gym-coach", "app-1-2-4", t0), Err("busy"));
    // Another app is not held up by it.
    live.connection("timer-app", true);
    live.begin_call("timer-app", "app-1-2-5", t0).unwrap();
    live.expire_calls(t0 + Duration::from_millis(4_499));
    assert!(live.drain_stopped().is_empty());
    assert!(live.drain_called().is_empty());
    live.expire_calls(t0 + crate::app_yes::CALL_FOR);
    assert_eq!(live.drain_stopped().len(), 2);
    let told = live.drain_called();
    assert_eq!(told.len(), 2);
    assert!(
        told.iter()
            .all(|called| called.error.as_deref() == Some("timeout"))
    );
    let answer = frame(json!({"id": 4, "op": "app.answer", "call": "app-1-2-3", "result": {}}));
    let reply = answer_op(
        &mut live,
        "gym-coach",
        &answer,
        4,
        true,
        t0 + Duration::from_secs(6),
    );
    assert_eq!(reply["taken"], false, "late");
    assert!(live.drain_called().is_empty());
    // The app is free for the next call.
    live.begin_call("gym-coach", "app-1-2-6", t0 + Duration::from_secs(6))
        .unwrap();
}

#[test]
fn a_call_in_flight_ends_with_its_apps_last_connection_as_timeout_at_once() {
    let t0 = Instant::now();
    let mut live = in_flight(t0);
    live.connection("gym-coach", true);
    live.connection("gym-coach", false);
    assert!(live.drain_called().is_empty(), "one connection is still up");
    live.connection("gym-coach", false);
    let told = live.drain_called();
    assert_eq!(told.len(), 1);
    // Never `stopped` or `not running`: the voice says those as "nothing
    // was done", and the app may have done it.
    assert_eq!(told[0].error.as_deref(), Some("timeout"));
}

#[test]
fn a_tool_is_known_only_from_a_fresh_apps_tools_naming_it_for_that_app() {
    let now = Instant::now();
    let tools: AppTools = serde_json::from_value(json!({"apps": [{
        "app": "gym-coach", "title": "Gym Coach", "since_ms": 1,
        "tools": [{"name": "set_plan", "description": "Set the plan.",
                   "parameters": {"type": "object", "properties": {}}}]
    }]}))
    .unwrap();
    let read = Some((tools, now));
    assert!(lists(read.as_ref(), "gym-coach", "set_plan", now));
    assert!(!lists(read.as_ref(), "gym-coach", "get_plan", now));
    assert!(!lists(read.as_ref(), "other-app", "set_plan", now));
    assert!(!lists(None, "gym-coach", "set_plan", now));
    assert!(!lists(
        read.as_ref(),
        "gym-coach",
        "set_plan",
        now + Duration::from_secs(30)
    ));
}

#[test]
fn the_switch_is_on_only_for_on() {
    set_on_for_test(None);
    // The test runner sets no LAMP_APP_TOOLS.
    assert!(!on() || std::env::var(ENVIRONMENT).as_deref() == Ok("on"));
    set_on_for_test(Some(true));
    assert!(on());
    set_on_for_test(None);
}

/// Twelve's P3 3 on #8496: the call in flight published again is
/// `ALREADY`, never `busy`, and leaves the first call as it is.
#[test]
fn the_same_call_again_leaves_the_first_alone() {
    let t0 = Instant::now();
    let mut live = in_flight(t0);
    assert_eq!(
        live.begin_call("gym-coach", "app-1-2-3", t0 + Duration::from_millis(100)),
        Err(crate::app_yes::ALREADY)
    );
    assert!(live.drain_called().is_empty());
    // Its wait still runs from the first send.
    live.expire_calls(t0 + crate::app_yes::CALL_FOR);
    assert_eq!(
        live.drain_stopped(),
        [("gym-coach".to_owned(), "app-1-2-3".to_owned())]
    );
}

/// Twelve's P3 2 on #8496: ten `app.answer` frames an app in any second,
/// a bad one counted too; the eleventh is refused `rate` with its wait, and
/// another app is not held up by it.
#[test]
fn an_app_sends_ten_answers_a_second_at_most() {
    let t0 = Instant::now();
    let mut live = in_flight(t0);
    let wrong = frame(json!({"id": 4, "op": "app.answer", "call": "app-1-2-9", "result": {}}));
    for n in 0..crate::app_yes::ANSWERS_A_SECOND {
        let at = t0 + Duration::from_millis(n as u64);
        let reply = answer_op(&mut live, "gym-coach", &wrong, 4, true, at);
        assert_eq!(reply["taken"], false, "{n}");
    }
    let right = frame(json!({"id": 5, "op": "app.answer", "call": "app-1-2-3", "result": {}}));
    let refused = answer_op(
        &mut live,
        "gym-coach",
        &right,
        5,
        true,
        t0 + Duration::from_millis(20),
    );
    assert_eq!(refused["error"]["code"], "rate", "{refused}");
    assert_eq!(refused["error"]["retry_ms"], 980);
    assert!(live.drain_called().is_empty(), "nothing told");
    live.connection("timer-app", true);
    let other = answer_op(
        &mut live,
        "timer-app",
        &wrong,
        6,
        true,
        t0 + Duration::from_millis(20),
    );
    assert_eq!(other["taken"], false);
    // A second after the first, it may answer again, and its call is taken.
    let later = answer_op(
        &mut live,
        "gym-coach",
        &right,
        7,
        true,
        t0 + Duration::from_secs(1),
    );
    assert_eq!(later["taken"], true);
}

/// Twelve's P3 4 on #8496: a call's journal word is the outcome, never the
/// result or the app's own words.
#[test]
fn a_calls_journal_word_never_carries_the_apps_words() {
    let called = |result: Option<Value>, error: Option<&str>| AppCalled {
        call: "app-1-2-3".into(),
        app: "gym-coach".into(),
        result,
        error: error.map(str::to_owned),
    };
    assert_eq!(outcome(&called(Some(json!({"sets": 3})), None)), "answered");
    assert_eq!(outcome(&called(None, Some("timeout"))), "timeout");
    assert_eq!(outcome(&called(None, Some("unknown tool"))), "unknown tool");
    assert_eq!(
        outcome(&called(None, Some("Anna's plan is full"))),
        "the app's error"
    );
}
