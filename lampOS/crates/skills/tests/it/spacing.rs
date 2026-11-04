//! `LAMP_WELLBEING_SPACING=on` (#5207): the wellbeing nudges at a present
//! spacing (water after 45 minutes and each hour after, a comfort break
//! after 90, two an hour between them and never two within 20 minutes),
//! the gates kept (quiet hours, not while talking), a run whose word was
//! never said giving its group's hour back, and Welcome back's morning
//! clause once a day. Off, every skill runs with its own values.
use lamp_skills::{Call, Engine, SPACING_ENVIRONMENT, Skill, load_switched};
use std::path::{Path, PathBuf};

const MINUTE_US: u64 = 60_000_000;
const ON: &[&str] = &[SPACING_ENVIRONMENT];
/// 2026-01-01 00:00 UTC, the local time with offset 0.
const MIDNIGHT_MS: u64 = 1_767_225_600_000;

/// The wellbeing group's members, every one of `lampOS/skills/`.
const WELLBEING: [&str; 8] = [
    "comfort-break",
    "meal-check",
    "rest-your-eyes",
    "sip-of-water",
    "sit-back",
    "stand-up",
    "stretch-break",
    "wind-down",
];

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

fn skill(name: &str, on: &[&str]) -> Skill {
    load_switched(&skills_root().join(name), on).unwrap()
}

/// Someone at the desk, a fifth of the picture to one side and back each
/// half minute, so each nudge is noticed.
fn face(half: u64) -> String {
    let left = if half.is_multiple_of(2) { 0.3 } else { 0.5 };
    format!(
        r#"{{"faces":[{{"id":"a1","xmin":{left},"ymin":0.2,"xmax":{},"ymax":0.6,"confidence":0.9}}]}}"#,
        left + 0.3
    )
}

/// `minutes` of someone sitting at the desk from `start` (local minutes of
/// the day), faces every 1.5 s (under their 2 s lifespan, as the eyes say
/// them) and the voice `voice` throughout: each run that made something,
/// by its minute from the start and its skill. `unsaid` is told each run
/// at once, as if its word expired unsaid.
fn sitting_with(
    engine: &mut Engine,
    start: u64,
    minutes: u64,
    voice: &str,
    mut ran: impl FnMut(&mut Engine, &str),
) -> Vec<(u64, String)> {
    let state = format!(r#"{{"state":"{voice}"}}"#);
    let mut nudged = Vec::new();
    for step in 0..=minutes * 40 {
        let at_us = step * 1_500_000;
        engine.clock(0, at_us, MIDNIGHT_MS + start * 60_000 + at_us / 1_000);
        engine.observe("voice/state", at_us, state.as_bytes(), None);
        let faces = face(step / 20);
        for run in engine.observe("humans/faces/tracked", at_us, faces.as_bytes(), None) {
            if !run.calls.is_empty() {
                ran(engine, &run.skill);
                nudged.push((step / 40, run.skill));
            }
        }
    }
    nudged
}

fn sitting(engine: &mut Engine, start: u64, minutes: u64, voice: &str) -> Vec<(u64, String)> {
    sitting_with(engine, start, minutes, voice, |_, _| {})
}

/// Off, each skill has its own values, as before #5207.
#[test]
fn off_the_wellbeing_skills_keep_their_own_values() {
    let sip = skill("sip-of-water", &[]);
    assert_eq!(sip.triggers[0].for_us, 90 * MINUTE_US);
    assert_eq!(sip.most_a_day, Some(3));
    let comfort = skill("comfort-break", &[]);
    assert_eq!(comfort.triggers[0].for_us, 120 * MINUTE_US);
    let welcome = skill("welcome-back", &[]);
    assert_eq!(welcome.triggers.len(), 1);
    for name in WELLBEING {
        let loaded = skill(name, &[]);
        assert_eq!(loaded.budget, Some(("wellbeing".to_owned(), 1)), "{name}");
        assert_eq!(loaded.budget_apart_us, 0, "{name}");
        // The environment of the test has no switch on: as `load` reads it.
        assert_eq!(
            lamp_skills::load(&skills_root().join(name)).unwrap(),
            loaded
        );
    }
}

/// On, the new values: water at 45 minutes and each hour after, six a day;
/// a comfort break at 90 minutes; two an hour, 20 minutes apart.
#[test]
fn on_the_wellbeing_skills_take_the_present_spacing() {
    let sip = skill("sip-of-water", ON);
    assert_eq!(sip.triggers[0].for_us, 45 * MINUTE_US);
    assert_eq!(sip.triggers[0].cooldown_us, 60 * MINUTE_US);
    assert_eq!(sip.most_a_day, Some(6));
    let comfort = skill("comfort-break", ON);
    assert_eq!(comfort.triggers[0].for_us, 90 * MINUTE_US);
    assert!(comfort.enabled);
    for name in WELLBEING {
        let loaded = skill(name, ON);
        assert_eq!(loaded.budget, Some(("wellbeing".to_owned(), 2)), "{name}");
        assert_eq!(loaded.budget_apart_us, 20 * MINUTE_US, "{name}");
    }
    let welcome = skill("welcome-back", ON);
    let names: Vec<_> = welcome
        .triggers
        .iter()
        .map(|trigger| trigger.name.as_deref().unwrap())
        .collect();
    assert_eq!(names, ["first_today", "arrival"]);
}

/// A long working day at the desk with every wellbeing nudge loaded: on, the
/// nudges come at the new spacing, never two within 20 minutes nor three in
/// an hour; off, one an hour as before.
#[test]
fn a_long_sitting_gets_nudges_twenty_minutes_apart_at_most_two_an_hour() {
    let all =
        |on: &[&str]| -> Vec<Skill> { WELLBEING.iter().map(|name| skill(name, on)).collect() };
    let mut engine = Engine::new(all(ON), true).spacing();
    // 10:00, five hours at the desk, the voice resting.
    let nudged = sitting(&mut engine, 600, 300, "resting");
    let minutes: Vec<u64> = nudged.iter().map(|(minute, _)| *minute).collect();
    for pair in minutes.windows(2) {
        assert!(pair[1] - pair[0] >= 20, "two within 20 minutes: {nudged:?}");
    }
    for (index, minute) in minutes.iter().enumerate() {
        let in_hour = minutes[index..]
            .iter()
            .take_while(|later| **later < minute + 60)
            .count();
        assert!(in_hour <= 2, "three in an hour from {minute}: {nudged:?}");
    }
    // Water first at 45 minutes (as Rest your eyes), and again in the sitting.
    let sips: Vec<u64> = nudged
        .iter()
        .filter(|(_, name)| name == "sip-of-water")
        .map(|(minute, _)| *minute)
        .collect();
    assert!(sips.len() >= 2, "{nudged:?}");
    assert!(sips[0] <= 70, "{nudged:?}");
    assert!(
        nudged.iter().any(|(_, name)| name == "comfort-break"),
        "{nudged:?}"
    );
    assert!(nudged.len() >= 8, "{nudged:?}");

    // Off: one an hour between them, as before.
    let mut engine = Engine::new(all(&[]), true);
    let before = sitting(&mut engine, 600, 300, "resting");
    let minutes: Vec<u64> = before.iter().map(|(minute, _)| *minute).collect();
    for pair in minutes.windows(2) {
        assert!(pair[1] - pair[0] >= 60, "{before:?}");
    }
    assert!(before.len() < nudged.len(), "{before:?} {nudged:?}");
    assert!(!before.iter().any(|(_, name)| name == "comfort-break"));
}

/// The gates still hold with the switch on: nothing while they talk with the
/// lamp, and nothing in the quiet hours (22:00 to 08:00), Wind down's own
/// evening hour aside, which is not quiet for it.
#[test]
fn on_the_nudges_keep_quiet_while_talking_and_at_night() {
    let all: Vec<Skill> = WELLBEING.iter().map(|name| skill(name, ON)).collect();
    let mut engine = Engine::new(all.clone(), true).spacing();
    assert_eq!(sitting(&mut engine, 600, 180, "listening"), []);
    let mut engine = Engine::new(all, true).spacing();
    // 22:00 to 01:00.
    let night = sitting(&mut engine, 1_320, 180, "resting");
    assert!(
        night.iter().all(|(_, name)| name == "wind-down"),
        "{night:?}"
    );
    assert!(night.len() <= 1, "{night:?}");
}

/// lamp-9adf, 4 Nov: Stretch break's word expired unsaid and still spent the
/// wellbeing hour, so the next nudge due was turned away. With the switch,
/// a run whose word was never said gives its place back; without, nothing
/// changes.
#[test]
fn a_word_never_said_gives_its_groups_hour_back_only_with_the_switch() {
    let pair = |on: &[&str]| vec![skill("rest-your-eyes", on), skill("sip-of-water", on)];
    for (on, spacing) in [(ON, true), (&[][..], false)] {
        let mut engine = Engine::new(pair(on), true);
        if spacing {
            engine = engine.spacing();
        }
        // Rest your eyes' word expires unsaid each time.
        let ran = sitting_with(&mut engine, 600, 130, "resting", |engine, name| {
            if name == "rest-your-eyes" {
                assert_eq!(engine.unsaid(name), spacing);
            }
        });
        let sip = ran
            .iter()
            .find(|(_, name)| name == "sip-of-water")
            .map(|(minute, _)| *minute);
        if spacing {
            // At 45 minutes, as soon as its own sitting allows, not held
            // 20 minutes behind a word never said.
            assert_eq!(sip, Some(45), "{ran:?}");
        } else {
            // At 90, but the hour is Rest your eyes', spent at 45.
            assert_eq!(sip, Some(105), "{ran:?}");
        }
    }
    // Nothing to give back for a skill of no group, or one unknown.
    let mut engine = Engine::new(vec![skill("welcome-back", ON)], true).spacing();
    assert!(!engine.unsaid("welcome-back"));
    assert!(!engine.unsaid("no-such-skill"));
}

/// Welcome back with the switch: the first arrival of the day, 05:00 to
/// 11:00, gets the hello and "what's first today?"; the next arrival only
/// the hello; never two hellos for one arrival. Kept in the daily journal.
#[test]
fn the_first_arrival_of_the_day_is_asked_whats_first_once() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        root.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let journal = crate::journal(root.path().join("daily.json"));
    let mut engine = Engine::new(vec![skill("welcome-back", ON)], true).with_daily_journal(journal);
    let mut hellos = Vec::new();
    // 07:00: nobody for ten minutes, there 30 minutes, away 30, back 30.
    for second in 0..6_000u64 {
        let at_us = second * 1_000_000;
        engine.clock(0, at_us, MIDNIGHT_MS + 7 * 3_600_000 + second * 1_000);
        engine.observe("voice/state", at_us, br#"{"state":"resting"}"#, Some(3_000));
        let there = matches!(second, 600..2_400 | 4_200..6_000);
        let faces = if there {
            r#"{"faces":[{"id":"a1","xmin":0.4,"ymin":0.2,"xmax":0.7,"ymax":0.5,"confidence":0.9}]}"#
        } else {
            r#"{"faces":[]}"#
        };
        for run in engine.observe("humans/faces/tracked", at_us, faces.as_bytes(), Some(2_000)) {
            // The hello is asked of the Policy even with the ask skipped:
            // its calls are what a yes makes.
            let calls = if run.ask.is_some() {
                engine.still(&run, at_us).unwrap_or_default()
            } else {
                run.calls.clone()
            };
            let said: Vec<String> = calls
                .iter()
                .filter(|call| matches!(call, Call::Say(_) | Call::Words(_)))
                .map(|call| format!("{call:?}"))
                .collect();
            if !said.is_empty() {
                hellos.push((second, said.join(" ")));
            }
        }
    }
    assert_eq!(hellos.len(), 2, "{hellos:?}");
    assert!(hellos[0].0 < 620, "{hellos:?}");
    assert!(hellos[0].1.contains("what's first today?"), "{hellos:?}");
    assert!(hellos[1].0 >= 4_200, "{hellos:?}");
    assert!(!hellos[1].1.contains("first today"), "{hellos:?}");
    assert!(
        hellos[1].1.contains("They are back at their desk"),
        "{hellos:?}"
    );
}

/// A nudge's word unsaid gives its hour back once, and only after a run:
/// a word after a nudge (`follows`) or a tool's run takes the chance away
/// (`Engine::unsaid`).
#[test]
fn a_nudges_unsaid_word_gives_its_hour_back_once() {
    let mut engine = Engine::new(vec![skill("rest-your-eyes", ON)], true).spacing();
    // Nothing run yet: nothing to give back.
    assert!(!engine.unsaid("rest-your-eyes"));
    let ran = sitting_with(&mut engine, 600, 50, "resting", |engine, name| {
        assert!(engine.unsaid(name));
        // Once: the same run is not given back twice.
        assert!(!engine.unsaid(name));
    });
    assert_eq!(ran, [(45, "rest-your-eyes".to_owned())]);
}
