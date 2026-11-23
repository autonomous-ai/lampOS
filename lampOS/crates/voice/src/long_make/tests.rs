use super::*;

const NOW: u64 = 1_800_000_000_000;

/// A folder of a test's own, removed when dropped.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn folder(name: &str) -> Folder {
    let path = std::env::temp_dir().join(format!("long-make-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Folder(path)
}

#[test]
fn off_unless_set_on() {
    assert_eq!(ENVIRONMENT, "LAMP_MAKE_MAY_RUN_ON");
    assert!(on_with(Some("on")));
    for off in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
        assert!(!on_with(off), "{off:?}");
    }
    assert_eq!(RUN_ON_AFTER, Duration::from_secs(60));
    assert_eq!(MAKE_WITHIN, lamp_tasks::skill_make::RUN_ON_MOST_WITHIN);
    assert!(WRITE_WITHIN < MAKE_WITHIN);
    assert_eq!(
        RUN_ON_LINE,
        "This one's taking a while. I'll tell you when it's ready."
    );
}

/// The first builder's lines (#6811, 17:23): the name spoken, its dashes
/// as spaces; with no name yet, "your skill".
#[test]
fn its_lines_name_the_skill_as_said() {
    assert_eq!(
        still_making_line(Some("water-reminder")),
        "I'm still making your water reminder skill."
    );
    assert_eq!(still_making_line(None), "I'm still making your skill.");
    assert_eq!(
        ready_line("water-reminder"),
        "Your water reminder skill is ready."
    );
    assert_eq!(
        not_finished_line(Some("water-reminder")),
        "I couldn't finish your water reminder skill."
    );
    assert_eq!(not_finished_line(None), "I couldn't finish your skill.");
}

/// A make is let run on only while it is under way: never one not begun
/// (the switch off, or no make), nor one that settled first; once let go,
/// its outcome is kept for its person ([`settle`]) and it is marked
/// `making` until then.
#[test]
fn only_a_make_under_way_is_let_run_on() {
    let dir = folder("let-go");
    assert!(!let_go(6_811_001, NOW), "never begun: the turn waits");
    begin(6_811_002, &dir.0);
    assert_eq!(settle(6_811_002), None, "settled first: the turn takes it");
    assert!(!let_go(6_811_002, NOW));
    assert_eq!(read(&dir.0, NOW), None);
    begin(6_811_003, &dir.0);
    hint(6_811_003, "sip-at-desk");
    assert!(let_go(6_811_003, NOW));
    assert!(is_let_go(6_811_003));
    assert!(!let_go(6_811_003, NOW), "let go once");
    assert_eq!(
        read(&dir.0, NOW + 1),
        Some(State::Making {
            began_ms: NOW,
            name: Some("sip-at-desk".into())
        })
    );
    assert_eq!(settle(6_811_003), Some(dir.0.clone()));
    assert!(!is_let_go(6_811_003));
}

/// One at a time: while one runs on, a make ask is told it is still
/// making, by its draft's name; none before nor after.
#[test]
fn a_second_ask_while_one_runs_on_is_told_its_still_making() {
    let dir = folder("second");
    assert_eq!(still_making(&dir.0, NOW), None);
    begin(6_811_010, &dir.0);
    hint(6_811_010, "water-reminder");
    assert!(let_go(6_811_010, NOW));
    let told = still_making(&dir.0, NOW + 30_000).unwrap();
    assert_eq!(
        told["next"],
        "Say only this, in the language of the conversation, and nothing else: I'm still making your water reminder skill."
    );
    settle(6_811_010);
    settled(&dir.0, Err(false), NOW + 60_000);
    assert_eq!(still_making(&dir.0, NOW + 60_000), None);
}

/// "Is my skill ready?" told from the state: still making and for how
/// long, made, not made, or none; its name only to the owner.
#[test]
fn is_my_skill_ready_answers_each_state() {
    let dir = folder("status");
    assert_eq!(status(&dir.0, NOW, true)["state"], "none_being_made");
    begin(6_811_020, &dir.0);
    hint(6_811_020, "water-reminder");
    assert!(let_go(6_811_020, NOW));
    let making = status(&dir.0, NOW + 150_000, true);
    assert_eq!(making["state"], "still_making");
    assert_eq!(making["minutes_so_far"], 3);
    assert_eq!(making["most_minutes"], 10);
    assert_eq!(making["name"], "water reminder");
    assert_eq!(status(&dir.0, NOW + 150_000, false)["name"], Value::Null);
    settle(6_811_020);
    settled(&dir.0, Ok(("water-reminder", "ab12")), NOW + 200_000);
    let made = status(&dir.0, NOW + 200_000, true);
    assert_eq!(made["state"], "made");
    assert_eq!(made["name"], "water reminder");
    std::fs::remove_file(dir.0.join(FILE)).unwrap();
    begin(6_811_021, &dir.0);
    assert!(let_go(6_811_021, NOW));
    settle(6_811_021);
    settled(&dir.0, Err(true), NOW + 600_000);
    assert_eq!(status(&dir.0, NOW + 600_000, true)["state"], "not_made");
    assert!(STATUS_NEXT.contains("still being made"));
}

/// A restart while it ran (its mark still `making`) is told as not
/// finished; so is a mark left past the ten minutes with nobody to settle
/// it.
#[test]
fn a_restart_or_a_run_out_mark_is_not_finished() {
    let dir = folder("restart");
    begin(6_811_030, &dir.0);
    assert!(let_go(6_811_030, NOW));
    after_start(&dir.0, NOW + 5_000);
    assert_eq!(
        read(&dir.0, NOW + 5_000),
        Some(State::NotMade {
            name: None,
            why: NotFinished::Restart,
            ended_ms: NOW + 5_000
        })
    );
    settle(6_811_030);
    let dir = folder("ran-out");
    begin(6_811_031, &dir.0);
    assert!(let_go(6_811_031, NOW));
    let late = NOW + MAKE_WITHIN.as_millis() as u64 + RAN_OUT_GRACE_MS + 1;
    assert!(matches!(
        read(&dir.0, late),
        Some(State::NotMade {
            why: NotFinished::RanOut,
            ..
        })
    ));
    settle(6_811_031);
}

/// The first builder (#6811, 17:23): said only to its person at the lamp,
/// their face seen, not mid-conversation, never at night, never to an
/// empty room.
#[test]
fn ready_is_told_only_to_its_person_at_an_idle_moment() {
    assert!(may_tell(true, true, false, true));
    assert!(!may_tell(false, true, false, true), "their face not seen");
    assert!(!may_tell(true, false, false, true), "mid-conversation");
    assert!(!may_tell(true, true, true, true), "at night");
    assert!(!may_tell(true, true, false, false), "an empty room");
}

/// The coordinator offers news once per [`OFFER_AGAIN_AFTER`], and none
/// while the make still runs.
#[test]
fn the_news_is_offered_once_and_again_only_if_not_taken() {
    let dir = folder("offered");
    let mut tells = Tells::at(dir.0.clone());
    let at = Instant::now();
    assert_eq!(tells.next(at, NOW), None);
    begin(6_811_040, &dir.0);
    assert!(let_go(6_811_040, NOW));
    assert_eq!(tells.next(at + READ_EVERY, NOW), None, "still making");
    settle(6_811_040);
    settled(&dir.0, Err(false), NOW + 1);
    let note = tells.next(at + READ_EVERY * 2, NOW + 2).unwrap();
    assert!(note.starts_with(OPENS), "{note}");
    assert!(note.contains("I couldn't finish your skill."), "{note}");
    assert_eq!(tells.next(at + READ_EVERY * 3, NOW + 3), None);
    assert!(
        tells
            .next(at + READ_EVERY * 2 + OFFER_AGAIN_AFTER, NOW + 4)
            .is_some()
    );
}

/// Twelve's P3 and El's P2 on #7774: a telling not taken is offered at
/// most [`MOST_OFFERS`] times, then left to the app's list; news is
/// looked for before anything else of the moment.
#[test]
fn a_telling_is_offered_at_most_three_times() {
    let dir = folder("cap");
    let mut tells = Tells::at(dir.0.clone());
    let at = Instant::now();
    assert!(!tells.has_news(at, NOW));
    begin(6_811_050, &dir.0);
    assert!(let_go(6_811_050, NOW));
    settle(6_811_050);
    settled(&dir.0, Err(false), NOW + 1);
    let mut offered = 0;
    for step in 0..20u32 {
        let now = at + READ_EVERY + OFFER_AGAIN_AFTER * step;
        if tells.has_news(now, NOW + 2) {
            assert!(tells.next(now, NOW + 2).is_some());
            offered += 1;
        }
    }
    assert_eq!(offered, MOST_OFFERS);
    // Kept beside the marks: a restart doesn't begin the count again
    // (Twelve's 19:40 P3).
    let mut restarted = Tells::at(dir.0.clone());
    assert!(!restarted.has_news(at, NOW + 3));
    assert_eq!(
        read(&dir.0, NOW + 3).map(|state| key(&state)),
        Some(Some(format!("not_made {}", NOW + 1))),
        "kept, not told"
    );
}

/// Each make keeps a mark of its own; past [`MOST_MARKS`], a make let run
/// on drops the oldest untold one, and no ask is refused for them.
#[test]
fn past_the_most_marks_the_oldest_is_left_to_the_list() {
    let dir = folder("most");
    for (at, request) in (6_811_060..6_811_060 + MOST_MARKS as u64 + 1).enumerate() {
        let now = NOW + at as u64 * 10;
        assert_eq!(still_making(&dir.0, now), None);
        begin(request, &dir.0);
        assert!(let_go(request, now));
        assert!(still_making(&dir.0, now).is_some());
        settle(request);
        settled(&dir.0, Err(false), now + 1);
    }
    let kept: Vec<_> = marks(&dir.0, NOW + 100).iter().filter_map(key).collect();
    assert_eq!(
        kept,
        [
            "not_made 1800000000011",
            "not_made 1800000000021",
            "not_made 1800000000031"
        ]
    );
}

/// El's P2 on #7774: a make handed to the agent with the long make's time
/// whose turn is cut before its minute is let run on, its outcome through
/// the gate; one never handed to it (DeepSeek's quick reaction) is not.
#[test]
fn a_cut_turn_lets_only_an_agent_make_run_on() {
    let dir = folder("cut");
    begin(6_811_070, &dir.0);
    assert!(!cut(6_811_070, NOW), "not the agent's: as before");
    to_agent(6_811_070);
    assert!(cut(6_811_070, NOW));
    assert!(matches!(read(&dir.0, NOW), Some(State::Making { .. })));
    assert_eq!(settle(6_811_070), Some(dir.0.clone()));
}

/// Heron's P3 on #7774: a turn cut before its minute marks the agent's
/// make `making` at the cut, not when it ends, so a restart in between
/// tells it as not finished; a turn that took its answer marks nothing.
#[test]
fn a_cut_turn_marks_the_make_making_at_the_cut() {
    // The guard marks at the lamp's own clock.
    let now = crate::app_make::now_unix_ms();
    let dir = folder("cut-at-drop");
    begin(6_811_080, &dir.0);
    to_agent(6_811_080);
    CutAtDrop::new(6_811_080).answered();
    assert_eq!(read(&dir.0, now), None, "answered: no cut");
    drop(CutAtDrop::new(6_811_080));
    assert!(matches!(read(&dir.0, now), Some(State::Making { .. })));
    after_start(&dir.0, now + 1_000);
    assert!(matches!(
        read(&dir.0, now + 1_000),
        Some(State::NotMade {
            why: NotFinished::Restart,
            ..
        })
    ));
    assert_eq!(settle(6_811_080), Some(dir.0.clone()));
    // Not handed to the agent: as before, nothing marked at the cut.
    let other = folder("cut-at-drop-quick");
    begin(6_811_081, &other.0);
    drop(CutAtDrop::new(6_811_081));
    assert_eq!(read(&other.0, now), None);
    assert_eq!(settle(6_811_081), None);
}
