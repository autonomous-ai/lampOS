//! The four seen reactions together (#6543, the owner's call on #7401, 19
//! Nov): Seen celebrating, a drink, eating and tired each say one line
//! each time lamp-seen's count of their kind rises, with no cooldown, day's
//! most, group's hour or gap, quiet hours or rest; never while the lamp
//! talks or listens (a rise then is said once it rests) or sleeps; one
//! function stops them; and a line never heard takes nothing.
use crate::skills::seen_more_yes;
use lamp_skills::{Call, Engine, Run, Skill, load, tools_file};
use std::path::{Path, PathBuf};

const SEEN: [&str; 4] = ["seen-celebrate", "seen-drink", "seen-eat", "seen-tired"];

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

/// The seen reactions named, loaded and turned on, as a lamp that has them
/// on would hold them.
fn seen(names: &[&str]) -> Vec<Skill> {
    names
        .iter()
        .map(|name| {
            let mut skill = load(&skills_root().join(name)).unwrap();
            assert!(!skill.enabled, "{name} comes off");
            skill.enabled = true;
            skill
        })
        .collect()
}

/// A sitting from 10:00 to play: every 30 s as lamp-seen says it, each
/// kind of `sightings` (minute, `drank`, `ate`, `tired` or `celebrate`)
/// newly seen at its minute, the face moving a fifth of the picture each
/// time; the voice listening through each of `talks` (from, to minutes)
/// and resting otherwise.
struct Sitting<'a> {
    minutes: u64,
    sightings: &'a [(u64, &'a str)],
    talks: &'a [(u64, u64)],
}

impl<'a> Sitting<'a> {
    fn of(minutes: u64, sightings: &'a [(u64, &'a str)]) -> Self {
        Self {
            minutes,
            sightings,
            talks: &[],
        }
    }
}

/// Plays `sitting` to `engine`, handing each half-minute's runs to `each`
/// with the engine and the time; each skill that said something, at its
/// minute.
fn play_on(
    engine: &mut Engine,
    sitting: &Sitting,
    mut each: impl FnMut(&mut Engine, u64, &[Run]),
) -> Vec<(u64, String)> {
    // 10:00 local at 0 s.
    engine.clock(0, 0, 36_000_000);
    engine.observe("voice/state", 0, br#"{"state":"resting"}"#, None);
    let (mut drank, mut ate, mut tired, mut celebrated) = (0, 0, 0, 0);
    let mut said = Vec::new();
    for half in 0..=sitting.minutes * 2 {
        let at = half * 30_000_000;
        let mut runs = Vec::new();
        for (from, to) in sitting.talks {
            let state = match half {
                _ if half == from * 2 => "listening",
                _ if half == to * 2 => "resting",
                _ => continue,
            };
            let state = format!(r#"{{"state":"{state}"}}"#);
            runs.extend(engine.observe("voice/state", at, state.as_bytes(), None));
        }
        for (minute, kind) in sitting.sightings {
            if minute * 2 == half {
                match *kind {
                    "drank" => drank += 1,
                    "ate" => ate += 1,
                    "tired" => tired += 1,
                    "celebrate" => celebrated += 1,
                    other => panic!("{other}"),
                }
            }
        }
        let left = if half % 2 == 0 { 0.3 } else { 0.5 };
        let faces = format!(
            r#"{{"faces":[{{"id":"a1","xmin":{left},"ymin":0.2,"xmax":{},"ymax":0.6,"confidence":0.9}}]}}"#,
            left + 0.3
        );
        let doing = format!(
            r#"{{"tired_s_ago":0,"tired_seen":{tired},"drank_seen":{drank},"ate_seen":{ate},"desk_work_s_ago":0,"desk_work":"computer","desk_s":0,"celebrate_seen":{celebrated}}}"#,
        );
        runs.extend(engine.observe("humans/faces/tracked", at, faces.as_bytes(), None));
        runs.extend(engine.observe(
            "expression/state",
            at + 2_000,
            br#"{"asleep":false}"#,
            Some(500),
        ));
        runs.extend(engine.observe(
            "humans/doing_more",
            at + 4_000,
            doing.as_bytes(),
            Some(90_000),
        ));
        for run in &runs {
            if run.calls.iter().any(|call| matches!(call, Call::Say(_))) {
                said.push((half / 2, run.skill.clone()));
            }
        }
        each(engine, at + 5_000, &runs);
    }
    said
}

/// [`play_on`] a new engine of `skills`, with the owner's seen_more yes
/// (#6543) kept for every one of them when `yes`, without which
/// `humans/doing_more` reaches none; and every call each made, at its
/// minute.
#[allow(clippy::type_complexity)]
fn play_calls(
    skills: Vec<Skill>,
    sitting: &Sitting,
    yes: bool,
) -> (Vec<(u64, String)>, Vec<(u64, String, Call)>) {
    let names: Vec<String> = skills.iter().map(|skill| skill.name.clone()).collect();
    let mut engine = Engine::new(skills, true);
    let names: Vec<&str> = if yes {
        names.iter().map(String::as_str).collect()
    } else {
        Vec::new()
    };
    let _yes = seen_more_yes(&mut engine, &names);
    let mut called = Vec::new();
    let said = play_on(&mut engine, sitting, |_, at, runs| {
        for run in runs {
            for call in &run.calls {
                called.push((at / 60_000_000, run.skill.clone(), call.clone()));
            }
        }
    });
    (said, called)
}

fn play(skills: Vec<Skill>, sitting: &Sitting) -> Vec<(u64, String)> {
    play_calls(skills, sitting, true).0
}

fn named(said: &[(u64, String)]) -> Vec<(u64, &str)> {
    said.iter()
        .map(|(minute, skill)| (*minute, skill.as_str()))
        .collect()
}

#[test]
fn the_seen_reactions_come_off_with_no_limit_and_one_function_to_stop_them() {
    let skills = seen(&SEEN);
    for skill in &skills {
        let name = &skill.name;
        assert_eq!(skill.budget, None, "{name}: no group's hour or gap");
        assert!(skill.rest.is_none(), "{name}: no rest");
        assert_eq!(skill.most_a_day, None, "{name}: no day's most");
        assert_eq!(skill.triggers.len(), 1, "{name}: one trigger, one line");
        let trigger = &skill.triggers[0];
        assert_eq!(trigger.cooldown_us, 0, "{name}: no cooldown");
        assert!(trigger.wait_us > 0, "{name}: a rise mid-turn waits");
        assert!(skill.permissions.speak, "{name}");
        assert_eq!(
            skill.permissions.topics,
            [
                "humans/doing_more",
                "voice/state",
                "expression/state",
                "humans/faces/tracked"
            ],
            "{name}: no topic it no longer reads"
        );
        let stop = skill.stop.as_ref().unwrap();
        assert_eq!(
            (stop.tool.as_str(), stop.group.as_deref()),
            ("seen_remarks", Some("seen-remarks")),
            "{name}"
        );
    }
    let tools = tools_file(&skills);
    let functions: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(functions, ["seen_remarks"], "{tools}");
}

#[test]
fn each_one_seen_has_its_line_whatever_came_just_before() {
    // A minute apart, and twice in a row of a kind: a line each.
    let sightings = [
        (10, "drank"),
        (11, "ate"),
        (12, "celebrate"),
        (13, "tired"),
        (14, "drank"),
        (16, "drank"),
    ];
    let said = play(seen(&SEEN), &Sitting::of(20, &sightings));
    assert_eq!(
        named(&said),
        [
            (10, "seen-drink"),
            (11, "seen-eat"),
            (12, "seen-celebrate"),
            (13, "seen-tired"),
            (14, "seen-drink"),
            (16, "seen-drink")
        ]
    );
}

#[test]
fn a_rise_while_the_lamp_talks_is_said_once_it_rests() {
    // Seen at 10 in a talk from 9 to 14: said at 14, though lamp-seen's
    // count stopped rising four minutes before. Two seen in one talk are
    // one line after it.
    for kind in ["drank", "ate", "tired", "celebrate"] {
        let sightings = [(10, kind)];
        let sitting = Sitting {
            talks: &[(9, 14)],
            ..Sitting::of(20, &sightings)
        };
        let said = play(seen(&SEEN), &sitting);
        assert_eq!(said.len(), 1, "{kind}: {said:?}");
        assert_eq!(said[0].0, 14, "{kind}: {said:?}");
    }
    let sightings = [(10, "drank"), (12, "drank")];
    let sitting = Sitting {
        talks: &[(9, 14)],
        ..Sitting::of(20, &sightings)
    };
    assert_eq!(
        named(&play(seen(&["seen-drink"]), &sitting)),
        [(14, "seen-drink")]
    );
}

#[test]
fn a_line_never_heard_takes_nothing() {
    // The drink's lines at 10 and 12 are dropped unheard (the voice's
    // `no_audio_answer`: `main` gives the run back with `Engine::unsaid`);
    // eating's at 11 is heard. Each one seen still has its line, and a
    // "stop that" at 13 stops eating, the one heard last, not the drink.
    let sightings = [(10, "drank"), (11, "ate"), (12, "drank"), (14, "drank")];
    let mut engine = Engine::new(seen(&SEEN), true);
    let _yes = seen_more_yes(&mut engine, &SEEN);
    let mut stopped = None;
    let said = play_on(
        &mut engine,
        &Sitting::of(16, &sightings),
        |engine, at, runs| {
            for run in runs {
                if run.skill == "seen-drink" && run.calls.iter().any(|c| matches!(c, Call::Say(_)))
                {
                    assert_eq!(
                        engine.unsaid(&run.skill, at),
                        None,
                        "no group's hour to give"
                    );
                }
            }
            if at / 30_000_000 == 26 {
                let run = engine
                    .tool("seen_remarks", &serde_json::json!({ "how": "snooze" }), at)
                    .unwrap();
                stopped = Some((
                    engine.stopped("seen-eat", at),
                    engine.stopped("seen-drink", at),
                    run.calls,
                ));
            }
        },
    );
    assert_eq!(
        named(&said),
        [
            (10, "seen-drink"),
            (11, "seen-eat"),
            (12, "seen-drink"),
            (14, "seen-drink")
        ]
    );
    let (eat, drink, calls) = stopped.unwrap();
    assert!(eat && !drink, "{calls:?}");
    // Without the give-back, the drink at 12 would have been the one that
    // came last.
    let mut engine = Engine::new(seen(&SEEN), true);
    let _yes = seen_more_yes(&mut engine, &SEEN);
    let mut stopped = None;
    play_on(
        &mut engine,
        &Sitting::of(14, &sightings),
        |engine, at, _| {
            if at / 30_000_000 == 26 {
                engine
                    .tool("seen_remarks", &serde_json::json!({ "how": "snooze" }), at)
                    .unwrap();
                stopped = Some((
                    engine.stopped("seen-eat", at),
                    engine.stopped("seen-drink", at),
                ));
            }
        },
    );
    assert_eq!(stopped, Some((false, true)));
}

#[test]
fn without_the_owners_seen_more_yes_no_seen_reaction_makes_a_call() {
    // Every kind seen: with the owner's seen_more yes (#7529, kept by the
    // app per skill) each has its line; without it none makes a call of
    // any kind, no say, no gaze, nothing.
    let sightings = [(10, "drank"), (20, "ate"), (30, "tired"), (40, "celebrate")];
    let sitting = Sitting::of(45, &sightings);
    let said = play(seen(&SEEN), &sitting);
    assert_eq!(
        named(&said),
        [
            (10, "seen-drink"),
            (20, "seen-eat"),
            (30, "seen-tired"),
            (40, "seen-celebrate")
        ],
        "with the yes"
    );
    for name in SEEN {
        let (said, called) = play_calls(seen(&[name]), &sitting, false);
        assert!(said.is_empty() && called.is_empty(), "{name}: {called:?}");
    }
}
