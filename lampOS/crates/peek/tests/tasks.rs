//! The Tasks section (#7208, step 3): each Harness task the lamp handed
//! over, from `tasks/status` only, as a card with its engine and state.
use lamp_msgs::{
    AgentState, Sign, SkillTask, SkillTaskStage, TaskProgress, TaskSettled, TaskStatus,
};
use lamp_peek::{page, timeline::Timeline};

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
    // Working and just handed over.
    let mut sample = sample();
    sample
        .tasks
        .push(progress(4, "openclaw", AgentState::Working));
    sample.tasks.push(progress(5, "claude", AgentState::Idle));
    assert_eq!(
        page::tasks_line(Some(&sample)),
        "2 working (OpenClaw 2, Hermes 0, Claude 0), 1 just handed over"
    );
    // Working only.
    sample.unread.clear();
    assert_eq!(
        page::tasks_line(Some(&sample)),
        "2 working (OpenClaw 2, Hermes 0, Claude 0)"
    );
    // Only just handed over: no empty brackets.
    assert_eq!(
        page::tasks_line(Some(&status(vec![], vec![3], vec![]))),
        "none working, 1 just handed over"
    );
    // None, with or without ended ones.
    assert_eq!(
        page::tasks_line(Some(&status(vec![], vec![], vec![]))),
        "none working"
    );
    let mut ended = self::sample();
    ended.tasks.clear();
    ended.unread.clear();
    assert_eq!(page::tasks_line(Some(&ended)), "none working");
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

/// What `skills/tasks` says of a skill's run (#7873).
fn skill_news(stage: SkillTaskStage, run: Option<&str>) -> SkillTask {
    SkillTask {
        skill: "mail-check".into(),
        stage,
        run: run.map(str::to_owned),
        engine: (stage == SkillTaskStage::Started).then_some("openclaw".into()),
        gate: (stage == SkillTaskStage::Held).then_some("not alone".into()),
    }
}

/// #7873 (a), lamp-52e6 on 19 Nov: a skill's `do.task` ran on OpenClaw
/// (task-1795056147468-853891-0, 09:42:27 to 09:44:26) while Tasks said
/// "Nothing is being worked on": `tasks/status` lists only the store's
/// tasks. Its run shows with its engine and state, working, then done.
#[test]
fn a_skills_task_shows_on_the_tasks_card_with_its_engine_and_state() {
    let mut timeline = Timeline::new(false);
    let run = Some("task-1795056147468-853891-0");
    timeline.skill_task(skill_news(SkillTaskStage::Started, run), NOW - 60_000);
    let empty = status(vec![], vec![], vec![]);
    let html = page::tasks_with(Some(&empty), &timeline.skill_runs(), NOW);
    assert!(!html.contains("Nothing is being worked on."), "{html}");
    assert_eq!(html.matches("class=\"card task\"").count(), 1, "{html}");
    assert!(
        html.contains("<b>task 1795056147468-853891-0</b>"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"badge\">OpenClaw</span>"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"badge badge-blue\">working</span>"),
        "{html}"
    );
    assert!(
        html.contains("skill mail-check · began 1 min ago"),
        "{html}"
    );
    // Its end goes to its own run.
    timeline.skill_task(skill_news(SkillTaskStage::Ran, run), NOW - 20_000);
    let html = page::tasks_with(Some(&empty), &timeline.skill_runs(), NOW);
    assert!(html.contains("Nothing is being worked on."), "{html}");
    assert!(html.contains("class=\"card task ended\""), "{html}");
    assert!(
        html.contains("<span class=\"badge badge-green\">done</span>"),
        "{html}"
    );
    // A failed one, red; one that doesn't check is not kept.
    let other = Some("task-1795056379190-853891-1");
    timeline.skill_task(skill_news(SkillTaskStage::Started, other), NOW - 10_000);
    timeline.skill_task(skill_news(SkillTaskStage::Failed, other), NOW - 5_000);
    let mut hostile = skill_news(SkillTaskStage::Started, Some("<script>"));
    hostile.engine = Some("<script>".into());
    timeline.skill_task(hostile, NOW);
    let runs = timeline.skill_runs();
    assert_eq!(runs.len(), 2, "{runs:?}");
    let html = page::tasks_with(None, &runs, NOW);
    assert!(html.contains("No word from lamp-tasks"), "{html}");
    assert!(
        html.contains("<span class=\"badge badge-red\">failed</span>"),
        "{html}"
    );
    assert!(!html.contains("<script>"), "{html}");
}

/// #7873 (c): a skill's answer held at its gate (lamp-skills "waits for
/// its gate: not alone") shows "held for its gate: <why>", then told or
/// dropped; a read's answer, with no run, the same on a card of its own.
#[test]
fn a_skills_answer_held_at_its_gate_shows_why_then_told_or_dropped() {
    let mut timeline = Timeline::new(false);
    let run = Some("task-1795056147468-853891-0");
    timeline.skill_task(skill_news(SkillTaskStage::Started, run), NOW - 120_000);
    timeline.skill_task(skill_news(SkillTaskStage::Ran, run), NOW - 60_000);
    timeline.skill_task(skill_news(SkillTaskStage::Held, None), NOW - 50_000);
    let html = page::tasks_with(None, &timeline.skill_runs(), NOW);
    assert!(html.contains("held for its gate: not alone"), "{html}");
    assert!(!html.contains("then told"), "{html}");
    timeline.skill_task(skill_news(SkillTaskStage::Told, None), NOW - 10_000);
    let html = page::tasks_with(None, &timeline.skill_runs(), NOW);
    assert!(
        html.contains("held for its gate: not alone, then told"),
        "{html}"
    );
    assert_eq!(timeline.skill_runs().len(), 1);
    // A read's answer, held and dropped, is a card of its own.
    let mut held = skill_news(SkillTaskStage::Held, None);
    held.skill = "inbox-read".into();
    held.gate = Some("a voice heard".into());
    timeline.skill_task(held, NOW - 5_000);
    let mut dropped = skill_news(SkillTaskStage::Dropped, None);
    dropped.skill = "inbox-read".into();
    timeline.skill_task(dropped, NOW);
    let runs = timeline.skill_runs();
    assert_eq!(runs.len(), 2, "{runs:?}");
    let html = page::tasks_with(None, &runs, NOW);
    assert!(
        html.contains("<b>an answer</b>") && html.contains("skill inbox-read"),
        "{html}"
    );
    assert!(
        html.contains("held for its gate: a voice heard, then dropped"),
        "{html}"
    );
    // Told at once, never held: told alone.
    let mut once = Timeline::new(false);
    once.skill_task(skill_news(SkillTaskStage::Told, None), NOW);
    let html = page::tasks_with(None, &once.skill_runs(), NOW);
    assert!(html.contains("· told</div>"), "{html}");
}
