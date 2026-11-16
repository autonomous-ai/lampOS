//! The Tasks section (#7208, step 3): each Harness task the lamp handed
//! over, from `tasks/status` only, as a card with its engine and state.
use lamp_msgs::{AgentState, Sign, TaskProgress, TaskSettled, TaskStatus};
use lamp_peek::page;

const NOW: u64 = 1_800_000_000_000;

fn progress(task: u64, engine: &str, state: AgentState) -> TaskProgress {
    TaskProgress {
        task,
        engine: engine.into(),
        state,
        working_s: Some(300),
        last_active_unix_ms: Some(NOW - 20_000),
        read_unix_ms: NOW - 1_000,
        changes: 3,
        changed: true,
        new_share: 0.5,
        signs: vec![Sign::TestsPass],
        signs_seen: vec![Sign::TestsPass],
    }
}

fn status(tasks: Vec<TaskProgress>, unread: Vec<u64>, settled: Vec<TaskSettled>) -> TaskStatus {
    TaskStatus {
        store_id: "0123456789abcdef0123456789abcdef".into(),
        run: NOW - 600_000,
        tasks,
        unread,
        settled,
    }
}

fn sample() -> TaskStatus {
    status(
        vec![progress(1, "openclaw", AgentState::Working)],
        vec![3],
        vec![TaskSettled {
            task: 2,
            engine: "hermes".into(),
            state: AgentState::Idle,
            settled: true,
            at_unix_ms: NOW - 90_000,
        }],
    )
}

#[test]
fn a_working_a_settled_and_an_unread_task_are_three_cards_with_their_badges() {
    let html = page::tasks(Some(&sample()), NOW);
    assert_eq!(html.matches("class=\"card task").count(), 3, "{html}");
    // OpenClaw, working, blue, started 5 min ago, its changes and sign.
    assert!(
        html.contains("<span class=\"badge\">OpenClaw</span>"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"badge badge-blue\">working</span>"),
        "{html}"
    );
    assert!(html.contains("started 5 min ago"), "{html}");
    assert!(html.contains("last active 20 s ago"), "{html}");
    assert!(html.contains("3 changes"), "{html}");
    assert!(
        html.contains("<span class=\"chip\">tests pass</span>"),
        "{html}"
    );
    // The unread one: just handed over.
    assert!(html.contains("just handed over"), "{html}");
    // Hermes, settled idle: done, green, greyed under Ended.
    let (_, ended) = html.split_once("<h3>Ended</h3>").expect(&html);
    assert!(ended.contains("class=\"card task ended\""), "{ended}");
    assert!(
        ended.contains("<span class=\"badge\">Hermes</span>"),
        "{ended}"
    );
    assert!(
        ended.contains("<span class=\"badge badge-green\">done</span>"),
        "{ended}"
    );
    assert!(ended.contains("1 min ago"), "{ended}");
}

#[test]
fn each_state_has_its_colour() {
    for (state, class) in [
        (AgentState::Working, "badge-blue"),
        (AgentState::Starting, "badge-amber"),
        (AgentState::Installing, "badge-amber"),
        (AgentState::NeedsInput, "badge-amber"),
        (AgentState::Idle, "badge-green"),
        (AgentState::Failed, "badge-red"),
        (AgentState::Stopped, "badge-grey"),
        (AgentState::Missing, "badge-grey"),
    ] {
        let html = page::tasks(
            Some(&status(vec![progress(1, "hermes", state)], vec![], vec![])),
            NOW,
        );
        assert!(
            html.contains(&format!("badge {class}")),
            "{state:?}: {html}"
        );
    }
}

#[test]
fn nothing_handed_over_says_so() {
    let html = page::tasks(Some(&status(vec![], vec![], vec![])), NOW);
    assert!(html.contains("Nothing is being worked on."), "{html}");
    assert!(!html.contains("class=\"card task"), "{html}");
    let html = page::tasks(None, NOW);
    assert!(html.contains("No word from lamp-tasks"), "{html}");
}

#[test]
fn the_overview_line_counts_working_tasks_per_engine() {
    let mut sample = sample();
    sample
        .tasks
        .push(progress(4, "openclaw", AgentState::Working));
    sample.tasks.push(progress(5, "claude", AgentState::Idle));
    assert_eq!(
        page::tasks_line(Some(&sample)),
        "2 working (OpenClaw 2, Hermes 0, Claude 0)"
    );
    assert_eq!(
        page::tasks_line(Some(&status(vec![], vec![], vec![]))),
        "none"
    );
    assert_eq!(page::tasks_line(None), "not known");
}

#[test]
fn a_hostile_engine_is_escaped() {
    let hostile = "<script>alert(1)</script>";
    let mut sample = sample();
    sample.tasks[0].engine = hostile.into();
    sample.settled[0].engine = hostile.into();
    let html = page::tasks(Some(&sample), NOW);
    assert!(!html.contains("<script>"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
    let line = page::tasks_line(Some(&sample));
    assert!(!line.contains("<script>"), "{line}");
}
