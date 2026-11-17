//! `tasks/status`: how the work handed over is going, as typed state.
use lamp_bus::Kind;
use lamp_msgs::{
    AgentState, Decision, Sign, StepKind, TASK_STATUS_MOST_TASKS, TASK_STEP_MOST_RESULT_BYTES,
    TaskProgress, TaskSettled, TaskStatus, TaskStep, cut_whole_chars, declaration_of, first_topics,
    tasks_topics,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn progress() -> TaskProgress {
    TaskProgress {
        task: 3,
        engine: "claude".into(),
        state: AgentState::Working,
        working_s: Some(754),
        last_active_unix_ms: Some(1_791_964_800_000),
        read_unix_ms: 1_791_964_812_345,
        changes: 4,
        changed: true,
        new_share: 0.5,
        signs: vec![Sign::TestsPass],
        signs_seen: vec![Sign::TestsPass, Sign::Found],
    }
}

fn status() -> TaskStatus {
    TaskStatus {
        store_id: "0123456789abcdef0123456789abcdef".into(),
        run: 1_791_964_000_000,
        unread: vec![],
        settled: Vec::new(),
        tasks: vec![progress()],
    }
}

#[test]
fn a_status_is_typed_state_without_words_and_ignores_fields_added_later() {
    let expected = status();
    expected.check().unwrap();
    let text = serde_json::to_string(&expected).unwrap();
    assert_eq!(
        text,
        r#"{"store_id":"0123456789abcdef0123456789abcdef","run":1791964000000,"tasks":[{"task":3,"engine":"claude","state":"working","working_s":754,"last_active_unix_ms":1791964800000,"read_unix_ms":1791964812345,"changes":4,"changed":true,"new_share":0.5,"signs":["tests_pass"],"signs_seen":["tests_pass","found"]}]}"#
    );
    assert_eq!(serde_json::from_str::<TaskStatus>(&text).unwrap(), expected);
    let mut value: Value = serde_json::from_str(&text).unwrap();
    value["future"] = json!(1);
    value["tasks"][0]["future"] = json!("a newer field");
    assert_eq!(
        serde_json::from_value::<TaskStatus>(value).unwrap(),
        expected
    );
    // Not known is left out.
    let mut unknown = progress();
    unknown.working_s = None;
    unknown.last_active_unix_ms = None;
    let value = serde_json::to_value(&unknown).unwrap();
    assert!(value.get("working_s").is_none() && value.get("last_active_unix_ms").is_none());
    // A state or a sign this version does not know is not guessed.
    let mut value = serde_json::to_value(status()).unwrap();
    value["tasks"][0]["state"] = json!("thinking");
    assert!(serde_json::from_value::<TaskStatus>(value).is_err());
    let mut value = serde_json::to_value(status()).unwrap();
    value["tasks"][0]["signs_seen"] = json!(["shipped"]);
    assert!(serde_json::from_value::<TaskStatus>(value).is_err());
    // The run is required: counts mean nothing without it.
    let mut without_run = serde_json::to_value(status()).unwrap();
    without_run.as_object_mut().unwrap().remove("run");
    assert!(serde_json::from_value::<TaskStatus>(without_run).is_err());
    // Every other field has to be there.
    let value = serde_json::to_value(progress()).unwrap();
    for field in [
        "task",
        "engine",
        "state",
        "read_unix_ms",
        "changes",
        "changed",
        "new_share",
        "signs",
        "signs_seen",
    ] {
        let mut without = value.clone();
        without.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<TaskProgress>(without).is_err(),
            "{field}"
        );
    }
    assert_eq!(
        serde_json::to_value(AgentState::NeedsInput).unwrap(),
        json!("needs_input")
    );
    assert_eq!(
        serde_json::to_value(Sign::PullRequest).unwrap(),
        json!("pull_request")
    );
}

#[test]
fn the_check_holds_the_bounds_and_the_signs_to_their_sense() {
    let wrong = |change: fn(&mut TaskStatus)| {
        let mut value = status();
        change(&mut value);
        value.check().unwrap_err().0
    };
    assert!(wrong(|s| s.store_id = "XYZ".into()).contains("store_id"));
    assert!(wrong(|s| s.run = 0).contains("run"));
    assert!(
        wrong(|s| s.tasks = vec![progress(); TASK_STATUS_MOST_TASKS + 1]).contains("at most 16")
    );
    assert!(wrong(|s| s.tasks = vec![progress(), progress()]).contains("once each"));
    assert!(wrong(|s| s.tasks[0].task = 0).contains("positive"));
    assert!(wrong(|s| s.tasks[0].engine = "Claude Code".into()).contains("engine"));
    assert!(wrong(|s| s.tasks[0].new_share = 1.5).contains("new_share"));
    assert!(wrong(|s| s.tasks[0].new_share = f32::NAN).contains("new_share"));
    assert!(
        wrong(|s| s.tasks[0].signs_seen = vec![Sign::Found, Sign::TestsPass]).contains("in order")
    );
    assert!(wrong(|s| s.tasks[0].signs = vec![Sign::Done]).contains("also a sign seen"));
    assert!(wrong(|s| s.tasks[0].read_unix_ms = u64::MAX).contains("9999"));
    assert!(
        wrong(|s| {
            s.tasks[0].changes = 0;
        })
        .contains("counted")
    );
    // Nothing watched is a status too.
    let empty = TaskStatus {
        tasks: Vec::new(),
        ..status()
    };
    empty.check().unwrap();
    // Watched and not read yet (#793 review): apart from the tasks read,
    // once each, in order, and left out of the message when there is none.
    let waiting = TaskStatus {
        tasks: Vec::new(),
        unread: vec![3, 7],
        settled: Vec::new(),
        ..status()
    };
    waiting.check().unwrap();
    let text = serde_json::to_string(&waiting).unwrap();
    assert!(text.contains("\"unread\":[3,7]"), "{text}");
    assert!(!serde_json::to_string(&status()).unwrap().contains("unread"));
    let older: TaskStatus = serde_json::from_str(
        r#"{"store_id":"0123456789abcdef0123456789abcdef","run":1,"tasks":[]}"#,
    )
    .unwrap();
    assert!(older.unread.is_empty());
    assert!(wrong(|s| s.unread = vec![7, 3]).contains("unread"));
    assert!(wrong(|s| s.unread = vec![3, 3]).contains("unread"));
    assert!(wrong(|s| s.unread = vec![s.tasks[0].task]).contains("unread"));
}

/// #1021: a task let go because its agent settled is said apart, with its
/// settled state, for a skill to act on per task; left out when there is
/// none, and read as none from an older publisher.
#[test]
fn a_task_let_go_because_it_settled_is_said_apart_with_its_state() {
    let done = TaskSettled {
        task: 5,
        engine: "codex".into(),
        state: AgentState::Idle,
        settled: true,
        at_unix_ms: 1_791_964_900_000,
    };
    let with = TaskStatus {
        settled: vec![done.clone()],
        ..status()
    };
    with.check().unwrap();
    let text = serde_json::to_string(&with).unwrap();
    assert!(
        text.ends_with(r#""settled":[{"task":5,"engine":"codex","state":"idle","settled":true,"at_unix_ms":1791964900000}]}"#),
        "{text}"
    );
    assert_eq!(serde_json::from_str::<TaskStatus>(&text).unwrap(), with);
    assert!(
        !serde_json::to_string(&status())
            .unwrap()
            .contains("settled")
    );
    let older: TaskStatus = serde_json::from_str(
        r#"{"store_id":"0123456789abcdef0123456789abcdef","run":1,"tasks":[]}"#,
    )
    .unwrap();
    assert!(older.settled.is_empty());
    for state in [
        AgentState::NeedsInput,
        AgentState::Stopped,
        AgentState::Failed,
    ] {
        TaskStatus {
            settled: vec![TaskSettled {
                state,
                ..done.clone()
            }],
            ..status()
        }
        .check()
        .unwrap();
    }
    let refused = |change: &dyn Fn(&mut TaskStatus)| {
        let mut wrong = with.clone();
        change(&mut wrong);
        wrong.check().unwrap_err().0
    };
    assert!(refused(&|s| s.settled[0].state = AgentState::Working).contains("settled state"));
    assert!(refused(&|s| s.settled[0].settled = false).contains("says settled"));
    assert!(refused(&|s| s.settled[0].task = 0).contains("positive"));
    assert!(refused(&|s| s.settled[0].engine = "Codex".into()).contains("engine"));
    // Not among the watched, and once each, in order.
    assert!(refused(&|s| s.settled[0].task = 3).contains("not among the watched"));
    assert!(refused(&|s| s.unread = vec![5]).contains("not among the watched"));
    assert!(refused(&|s| s.settled.push(s.settled[0].clone())).contains("once each"));
}

#[test]
fn the_topic_is_kept_latest_apart_from_the_first_topics() {
    let declared = TaskStatus::declaration();
    assert_eq!(declared.type_name, "lamp_msgs/TaskStatus");
    assert_eq!((declared.kind, declared.period()), (Kind::Latest, None));
    assert_eq!(tasks_topics()[0], (TaskStatus::TOPIC, declared.clone()));
    assert_eq!(declaration_of("tasks/status"), Some(declared));
    assert!(
        first_topics()
            .iter()
            .all(|(topic, _)| *topic != TaskStatus::TOPIC)
    );
}

#[test]
fn a_progress_decision_names_its_work_and_an_attention_one_is_written_as_before() {
    let attention = Decision {
        about: "attention".into(),
        choice: "answer".into(),
        probability: 0.9,
        answers: BTreeMap::new(),
        by: "rule".into(),
        took_ms: 0,
        shadow: true,
        request: None,
        task: None,
        arrival: None,
        ask: None,
        proposal: None,
    };
    // Without a task the bytes are the ones written before the field was.
    assert_eq!(
        serde_json::to_string(&attention).unwrap(),
        "{\"about\":\"attention\",\"choice\":\"answer\",\"probability\":0.9,\"answers\":{},\"by\":\"rule\",\"took_ms\":0,\"shadow\":true}"
    );
    let progress = Decision {
        about: "progress".into(),
        choice: "say_now".into(),
        task: Some("0123456789abcdef0123456789abcdef:3:4@1791964000000".into()),
        arrival: None,
        ask: None,
        ..attention
    };
    progress.check().unwrap();
    let text = serde_json::to_string(&progress).unwrap();
    assert!(text.ends_with(",\"task\":\"0123456789abcdef0123456789abcdef:3:4@1791964000000\"}"));
    assert_eq!(serde_json::from_str::<Decision>(&text).unwrap(), progress);
    for task in [
        "0123456789abcdef0123456789abcdef:3",
        // Without its run, a count names no change.
        "0123456789abcdef0123456789abcdef:3:4",
        "0123456789abcdef0123456789abcdef:3:4@",
        "0123456789abcdef0123456789abcdef:3:@5",
        "0123456789abcdef0123456789abcdef:3:4@x",
        "0123456789ABCDEF0123456789ABCDEF:3:4@5",
        "0123456789abcdef0123456789abcdef:x:4@5",
        "0123456789abcdef0123456789abcdef:3:4:5@6",
        "",
    ] {
        let wrong = Decision {
            task: Some(task.into()),
            arrival: None,
            ask: None,
            ..progress.clone()
        };
        assert!(wrong.check().unwrap_err().0.contains("task"), "{task:?}");
    }
}

/// #7267: one step an agent took, declared ahead of its publisher.
#[test]
fn a_step_names_its_task_tool_and_a_short_result() {
    let step = TaskStep {
        task: 2,
        engine: "openclaw".into(),
        n: 1,
        at_unix_ms: 1_791_964_812_345,
        kind: StepKind::Reply,
        tool: "web_search".into(),
        args: "{\"q\":\"lamps\"}".into(),
        // 70 two-byte characters cut to 120 bytes: 60, none split.
        result: cut_whole_chars(&"\u{e9}".repeat(70), TASK_STEP_MOST_RESULT_BYTES).into(),
        ok: Some(true),
    };
    assert!(
        serde_json::to_string(&step)
            .unwrap()
            .contains("\"kind\":\"reply\"")
    );
    assert_eq!(step.result.chars().count(), 60);
    assert_eq!(cut_whole_chars("a\u{e9}", 2), "a");
    step.check().unwrap();
    let text = serde_json::to_string(&step).unwrap();
    assert_eq!(serde_json::from_str::<TaskStep>(&text).unwrap(), step);
    let refused = |change: &dyn Fn(&mut TaskStep)| {
        let mut wrong = step.clone();
        change(&mut wrong);
        wrong.check().is_err()
    };
    assert!(refused(&|s| s.tool = "x".repeat(41)));
    assert!(refused(&|s| s.result = "x".repeat(121)));
    assert!(refused(&|s| s.args = "x".repeat(121)));
    assert!(refused(&|s| s.n = 0));
    assert!(refused(&|s| s.engine = "Open Claw".into()));
    // Each kind: a call names its tool; a reply and an error may not.
    let of = |kind, tool: &str| TaskStep {
        kind,
        tool: tool.into(),
        ..step.clone()
    };
    of(StepKind::Call, "web_search").check().unwrap();
    assert!(of(StepKind::Call, "").check().is_err());
    of(StepKind::Reply, "").check().unwrap();
    of(StepKind::Error, "").check().unwrap();
    for (kind, word) in [
        (StepKind::Call, "call"),
        (StepKind::Reply, "reply"),
        (StepKind::Error, "error"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), word);
    }
    let declared = TaskStep::declaration();
    assert_eq!(declared.type_name, "lamp_msgs/TaskStep");
    assert_eq!(declared.kind, Kind::Event);
    assert!(tasks_topics().contains(&(TaskStep::TOPIC, declared.clone())));
    assert_eq!(declaration_of("tasks/step"), Some(declared));
}
