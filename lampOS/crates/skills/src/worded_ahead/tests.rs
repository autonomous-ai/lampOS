use super::*;
use serde_json::json;

const HOUR_MS: u64 = 60 * 60 * 1000;
const NOON_MS: u64 = 1_800_000_000_000;

fn kind(skill: &str) -> Kind {
    let pilot = crate::core_agent::pilot(skill).unwrap();
    Kind {
        pilot,
        task: &pilot.tasks[0],
    }
}

fn kinds() -> Vec<Kind> {
    Kind::all_of(|name| matches!(name, "sip-of-water" | "stretch-break"))
}

fn idle<'a>(now_ms: u64, quiet: &'a dyn Fn(&str) -> bool, allowance: &'a dyn Allowance) -> Now<'a> {
    Now {
        now_ms,
        talking: false,
        warm: false,
        quiet,
        allowance,
    }
}

fn never(_: &str) -> bool {
    false
}

/// An allowance already spent.
struct Spent;

impl Allowance for Spent {
    fn allows(&self) -> bool {
        false
    }
    fn spent(&self, _run: Duration, _cpu_s: Option<f32>) {}
}

/// The cache fills when idle: with the lamp idle, a refill is picked, the
/// kind with no line first, and its line, kept, is the next fire's.
#[test]
fn the_cache_fills_when_idle() {
    let mut kept = Kept::default();
    let now = idle(NOON_MS, &never, &NoAllowanceYet);
    let first = kept.next_refill(&kinds(), &now).unwrap();
    assert_eq!(first.key(), "sip-of-water");
    kept.count_refill(first.pilot.name, NOON_MS);
    let answer = json!({"ok": true, "answer": "A sip of water?"}).to_string();
    assert_eq!(kept.refilled(&first, &answer, NOON_MS, NOON_MS), Ok(()));
    assert_eq!(kept.lines["sip-of-water"][0].words, "A sip of water?");
    assert_eq!(kept.lines["sip-of-water"][0].worded_ms, NOON_MS);
    // A dash keeps nothing; busy gives the refill back.
    let dash = json!({"ok": true, "answer": "-"}).to_string();
    assert!(kept.refilled(&first, &dash, NOON_MS, NOON_MS).is_err());
    kept.count_refill("sip-of-water", NOON_MS + 2);
    let busy = json!({"ok": false, "error": "busy"}).to_string();
    assert_eq!(
        kept.refilled(&first, &busy, NOON_MS + 2, NOON_MS + 2),
        Err(BUSY)
    );
    assert_eq!(kept.refills_today("sip-of-water", NOON_MS + 2), 1);
    // The other skill, with none, comes next.
    let second = kept.next_refill(&kinds(), &now).unwrap();
    assert_eq!(second.key(), "stretch-break");
    assert_eq!(kept.fresh_of("sip-of-water", NOON_MS), 1);
    // Full at MOST_LINES: nothing more is worded.
    let sip = kind("sip-of-water");
    for at in 1..=3 {
        kept.keep(&sip, format!("line {at}"), NOON_MS + at);
    }
    assert_eq!(kept.fresh_of("sip-of-water", NOON_MS + 3), MOST_LINES);
    let only_sip = Kind::all_of(|name| name == "sip-of-water");
    let later = idle(NOON_MS + 3, &never, &NoAllowanceYet);
    assert_eq!(kept.next_refill(&only_sip, &later), Err(FULL));
}

/// A skill of two tasks keeps each kind on its own key, and its lines
/// together are at most MOST_LINES.
#[test]
fn a_skill_of_two_tasks_keeps_each_kind() {
    let pilot = crate::core_agent::pilot("sit-back").unwrap();
    let leaned = Kind::of("sit-back", pilot.tasks[0].task).unwrap();
    let sat = Kind::of("sit-back", pilot.tasks[1].task).unwrap();
    assert_eq!(leaned.key(), "sit-back:leaned_in");
    assert_eq!(sat.key(), "sit-back:sat_well");
    let mut kept = Kept::default();
    kept.keep(&leaned, "Maybe sit back a moment?".into(), NOON_MS);
    let now = idle(NOON_MS, &never, &NoAllowanceYet);
    let next = kept
        .next_refill(&Kind::all_of(|name| name == "sit-back"), &now)
        .unwrap();
    assert_eq!(next.key(), "sit-back:sat_well", "the kind with none first");
    for at in 1..=4 {
        kept.keep(&sat, format!("Thanks {at}"), NOON_MS + at);
    }
    assert_eq!(kept.fresh_of("sit-back", NOON_MS + 4), MOST_LINES);
    assert!(Kind::of("sit-back", "Something else.").is_none());
}

/// A fire takes the oldest fresh line, once; a stale one is never said and
/// an empty cache gives none, so the fire says its own.
#[test]
fn a_stale_or_empty_cache_gives_no_line() {
    let sip = kind("sip-of-water");
    let mut kept = Kept::default();
    assert_eq!(kept.take(&sip, NOON_MS), None, "empty");
    kept.keep(&sip, "old".into(), NOON_MS - FRESH_MS - 1);
    kept.keep(&sip, "older fresh".into(), NOON_MS - 2 * HOUR_MS);
    kept.keep(&sip, "newer fresh".into(), NOON_MS - HOUR_MS);
    let line = kept.take(&sip, NOON_MS).unwrap();
    assert_eq!(line.words, "older fresh", "the oldest fresh first");
    assert_eq!(kept.take(&sip, NOON_MS).unwrap().words, "newer fresh");
    assert_eq!(kept.take(&sip, NOON_MS), None, "each said once");
    // A line from after now (the clock set back) is not fresh.
    kept.keep(&sip, "from the future".into(), NOON_MS + HOUR_MS);
    assert_eq!(kept.take(&sip, NOON_MS), None);
    // All stale: none.
    kept.keep(&sip, "yesterday".into(), NOON_MS);
    assert_eq!(kept.take(&sip, NOON_MS + FRESH_MS), None);
}

/// None fills mid-conversation, nor in the skill's quiet hours.
#[test]
fn none_fills_mid_conversation_or_in_quiet_hours() {
    let kept = Kept::default();
    let talking = Now {
        talking: true,
        ..idle(NOON_MS, &never, &NoAllowanceYet)
    };
    assert_eq!(kept.next_refill(&kinds(), &talking), Err(TALKING));
    let all_quiet = |_: &str| true;
    let quiet = idle(NOON_MS, &all_quiet, &NoAllowanceYet);
    assert_eq!(kept.next_refill(&kinds(), &quiet), Err(QUIET));
    // One skill quiet: the other is refilled.
    let sip_quiet = |name: &str| name == "sip-of-water";
    let one = idle(NOON_MS, &sip_quiet, &NoAllowanceYet);
    assert_eq!(
        kept.next_refill(&kinds(), &one).unwrap().key(),
        "stretch-break"
    );
}

/// The heat pause: `board/heat` warm or hot pauses refills while it is
/// fresh, and decides alone while it is; without it, `board/state` over
/// 84 °C does.
#[test]
fn a_warm_board_pauses_refills() {
    let kept = Kept::default();
    let warm_now = Now {
        warm: true,
        ..idle(NOON_MS, &never, &NoAllowanceYet)
    };
    assert_eq!(kept.next_refill(&kinds(), &warm_now), Err(WARM));
    let level = |level: &str| json!({"level": level, "zone": "cpul_thermal_zone", "c": 85.0, "trip_c": 90.0});
    let state = |c: f64| json!({"temperature_c": {"cpu-thermal": 60.0, "cpul_thermal_zone": c}});
    assert!(warm(Some((&level("warm"), 1_000)), None));
    assert!(warm(Some((&level("hot"), 1_000)), None));
    assert!(
        !warm(Some((&level("ok"), 1_000)), Some((&state(90.0), 1_000))),
        "board/heat alone decides"
    );
    assert!(warm(None, Some((&state(84.5), 1_000))));
    assert!(!warm(None, Some((&state(84.0), 1_000))));
    // Stale readings say nothing: the newer one decides, or neither.
    assert!(warm(
        Some((&level("ok"), READING_LASTS_MS)),
        Some((&state(88.0), 2_000))
    ));
    assert!(!warm(None, Some((&state(95.0), READING_LASTS_MS))));
    assert!(!warm(None, None));
}

/// The daily cap: MOST_REFILLS_A_DAY a skill in any 24 hours, counted as
/// each begins, whatever comes of it; the allowance spent stops every
/// refill.
#[test]
fn the_daily_cap_holds() {
    let mut kept = Kept::default();
    let only_sip = Kind::all_of(|name| name == "sip-of-water");
    for at in 0..MOST_REFILLS_A_DAY as u64 {
        let now = idle(NOON_MS + at * HOUR_MS, &never, &NoAllowanceYet);
        let next = kept.next_refill(&only_sip, &now).unwrap();
        kept.count_refill(next.pilot.name, now.now_ms);
    }
    let later = NOON_MS + 5 * HOUR_MS;
    assert_eq!(
        kept.refills_today("sip-of-water", later),
        MOST_REFILLS_A_DAY
    );
    assert_eq!(
        kept.next_refill(&only_sip, &idle(later, &never, &NoAllowanceYet)),
        Err(CAPPED)
    );
    // A day after the first, one more.
    let next_day = NOON_MS + DAY_MS;
    assert!(
        kept.next_refill(&only_sip, &idle(next_day, &never, &NoAllowanceYet))
            .is_ok()
    );
    // The allowance spent: none.
    assert_eq!(
        Kept::default().next_refill(&kinds(), &idle(NOON_MS, &never, &Spent)),
        Err(ALLOWANCE_SPENT)
    );
}

/// The kept lines and the day's count outlive a restart; a file not read
/// is nothing kept.
#[test]
fn what_is_kept_outlives_a_restart() {
    let dir = std::env::temp_dir().join(format!("worded-ahead-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(FILE);
    let mut kept = Kept::default();
    kept.keep(&kind("sip-of-water"), "A sip?".into(), NOON_MS);
    kept.count_refill("sip-of-water", NOON_MS);
    kept.save(&path).unwrap();
    assert_eq!(Kept::load(&path), kept);
    std::fs::write(&path, "not json").unwrap();
    assert_eq!(Kept::load(&path), Kept::default());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A refill's line says `"ahead": true`; its answer is checked as a
/// fire's line, and never one for the owner alone.
#[test]
fn a_refills_line_is_marked_and_its_answer_checked() {
    let line = json!({"op": "skill_task", "skill": "sip-of-water"}).to_string();
    let marked: serde_json::Value = serde_json::from_str(&marked(&line)).unwrap();
    assert_eq!(marked["ahead"], true);
    assert_eq!(marked["skill"], "sip-of-water");
    let ok = |answer: &str| json!({"ok": true, "answer": answer}).to_string();
    assert_eq!(
        checked(&ok("Fancy a sip of water?")),
        Ok("Fancy a sip of water?".into())
    );
    assert_eq!(checked(&ok("-")), Err(crate::core_agent::NOTHING_TO_SAY));
    assert_eq!(
        checked(&ok(&"word ".repeat(80))),
        Err(crate::core_agent::TOO_LONG)
    );
    let alone = json!({"ok": true, "answer": "Hi", "owner_alone": true}).to_string();
    assert_eq!(checked(&alone), Err(OWNER_ALONE));
    let refused = json!({"ok": false, "error": "cancelled"}).to_string();
    assert_eq!(checked(&refused), Err(YIELDED));
    let busy = json!({"ok": false, "error": "busy"}).to_string();
    assert_eq!(checked(&busy), Err(BUSY));
    // Busy: the refill given back, so the day's cap is never spent on it.
    let mut kept = Kept::default();
    kept.count_refill("sip-of-water", NOON_MS);
    kept.uncount_refill("sip-of-water", NOON_MS);
    assert_eq!(kept.refills_today("sip-of-water", NOON_MS), 0);
}

/// The journal gives the row what it counts: the agent's line or its own
/// per fire, and each refill's outcome, run time, CPU and count.
#[test]
fn the_journal_gives_the_rows_numbers() {
    let line = Line {
        words: "A sip?".into(),
        worded_ms: NOON_MS,
    };
    assert_eq!(
        said_line(Ok((&line, 3 * HOUR_MS))),
        "worded ahead: said its agent's line (6 characters, worded 3.0 h ago)"
    );
    assert_eq!(
        said_line(Err(NONE_FRESH)),
        "worded ahead: said its own line: no fresh line of its agent's kept"
    );
    assert_eq!(
        refill_line(
            &kind("sip-of-water"),
            Ok(()),
            Duration::from_millis(61_500),
            Some(48.25),
            2
        ),
        "worded ahead: refill of sip-of-water: its agent's line kept; run 61.5 s, cpu 48.2 s, refill 2 of 3 in 24 hours"
    );
    assert!(
        refill_line(
            &kind("stretch-break"),
            Err("cancelled"),
            Duration::ZERO,
            None,
            1
        )
        .contains("nothing kept: cancelled; run 0.0 s, cpu not read")
    );
    let dir = std::env::temp_dir().join(format!("worded-ahead-cpu-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cpu.stat"), "usage_usec 1234567\nuser_usec 1\n").unwrap();
    assert_eq!(cpu_usec(&dir.join("cpu.stat")), Some(1_234_567));
    assert_eq!(cpu_usec(&dir.join("none")), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Heron's HOLD on #7903: a refill's count is written before it is handed
/// over. Counted, then a crash (the file read again, the run never ended):
/// after three, the next is refused at the cap.
#[test]
fn the_count_outlives_a_crash_mid_refill() {
    let dir = std::env::temp_dir().join(format!("worded-ahead-crash-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(FILE);
    let only_sip = Kind::all_of(|name| name == "sip-of-water");
    for at in 0..MOST_REFILLS_A_DAY as u64 {
        // A new process each time: what the file keeps, and nothing else.
        let store = Store::at(Some(file.clone()));
        let now = idle(NOON_MS + at * HOUR_MS, &never, &NoAllowanceYet);
        let (kind, today) = store.begin_refill(&only_sip, &now).unwrap().unwrap();
        assert_eq!(kind.key(), "sip-of-water");
        assert_eq!(today, at as usize + 1);
        // The crash: dropped with the run under way, no end, no save.
        drop(store);
    }
    let store = Store::at(Some(file.clone()));
    let later = idle(NOON_MS + 5 * HOUR_MS, &never, &NoAllowanceYet);
    assert_eq!(store.begin_refill(&only_sip, &later), Ok(None));
    assert_eq!(store.kept().next_refill(&only_sip, &later), Err(CAPPED));
    // A count that can't be written begins nothing and is given back.
    let unwritable = Store::at(Some(dir.join("no-such-folder").join(FILE)));
    let now = idle(NOON_MS, &never, &NoAllowanceYet);
    assert!(unwritable.begin_refill(&only_sip, &now).is_err());
    assert_eq!(unwritable.kept().refills_today("sip-of-water", NOON_MS), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Heron's P3 on #7903: "forget everything" and a language change let the
/// kept lines go, written so; the day's count stays.
#[test]
fn forget_and_a_language_change_let_the_kept_lines_go() {
    let dir = std::env::temp_dir().join(format!("worded-ahead-forget-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(FILE);
    let store = Store::at(Some(file.clone()));
    let sip = kind("sip-of-water");
    assert!(
        !store.in_language(Some("en\n")),
        "the first read: nothing to let go"
    );
    assert!(!store.in_language(Some("en")), "the same language");
    store.kept().keep(&sip, "A sip?".into(), NOON_MS);
    store.kept().count_refill("sip-of-water", NOON_MS);
    assert!(store.clear());
    assert!(Kept::load(&file).lines.is_empty(), "written so");
    assert_eq!(Kept::load(&file).refills_today("sip-of-water", NOON_MS), 1);
    store.kept().keep(&sip, "A sip?".into(), NOON_MS);
    assert!(store.in_language(Some("vi")));
    assert!(store.kept().take(&sip, NOON_MS).is_none());
    assert_eq!(Kept::load(&file).language.as_deref(), Some("vi"));
    std::fs::remove_dir_all(&dir).unwrap();
}
