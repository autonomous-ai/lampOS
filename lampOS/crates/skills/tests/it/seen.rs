//! The four seen reactions together, as the product ships them (#6543, the
//! owner's call on #7401 and the first builder's rulings for the switch-on,
//! #7668): Seen celebrating, a
//! drink, eating and tired each say one line when lamp-seen's count of
//! their kind rises, with no day's most, group's hour or gap, or rest; the
//! same kind not again within 20 minutes of its last line heard (a
//! different kind may come sooner, and a line never heard costs nothing);
//! never while the lamp talks or listens (a rise then is said once it
//! rests, or dropped 3 minutes after it was seen), sleeps or with the
//! privacy slide closed, whose closing drops a kept rise at once; no quiet
//! hours, so a remark at night is said as normal; one function stops them;
//! and nothing they see leaves the lamp through them.
use crate::skills::seen_more_yes;
use lamp_skills::{Call, Engine, Permissions, Run, Skill, SkillKind, load, tools_file};
use std::path::{Path, PathBuf};

const SEEN: [&str; 4] = ["seen-celebrate", "seen-drink", "seen-eat", "seen-tired"];

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

/// The seen reactions named, loaded as the product installs them: core
/// skills, on.
fn seen(names: &[&str]) -> Vec<Skill> {
    names
        .iter()
        .map(|name| {
            let skill = load(&skills_root().join(name)).unwrap();
            assert!(skill.enabled, "{name} comes on");
            assert_eq!(skill.kind, SkillKind::Core, "{name}");
            skill
        })
        .collect()
}

/// A sitting from `starts` (local, in seconds of the day; 10:00 unless
/// said) to play: every 30 s as lamp-seen says it, each kind of
/// `sightings` (minute, `drank`, `ate`, `tired` (a yawn), `stretch` or
/// `celebrate`) newly seen
/// at its minute, the face moving a fifth of the picture each time; the
/// voice listening through each of `talks` (from, to minutes) and resting
/// otherwise; the privacy slide closed through each of `closed` (from, to
/// minutes) and open otherwise; each kind of `spells` (from, to minutes)
/// seen every 15 s through it, so its count rises by two at each word.
struct Sitting<'a> {
    minutes: u64,
    sightings: &'a [(u64, &'a str)],
    spells: &'a [(u64, u64, &'a str)],
    talks: &'a [(u64, u64)],
    closed: &'a [(u64, u64)],
    starts: u64,
}

impl<'a> Sitting<'a> {
    fn of(minutes: u64, sightings: &'a [(u64, &'a str)]) -> Self {
        Self {
            minutes,
            sightings,
            spells: &[],
            talks: &[],
            closed: &[],
            starts: 10 * 3600,
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
    // The sitting's start, local, at 0 s.
    engine.clock(0, 0, sitting.starts * 1_000);
    engine.observe("voice/state", 0, br#"{"state":"resting"}"#, None);
    let (mut drank, mut ate, mut tired, mut celebrated) = (0, 0, 0, 0);
    let (mut yawned, mut stretched) = (0, 0);
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
        let open = !sitting
            .closed
            .iter()
            .any(|(from, to)| (from * 2..to * 2).contains(&half));
        // Told by the program on a change, as `main` reads the bus (#7668).
        runs.extend(engine.privacy(!open, at + 1_000));
        let spelled = sitting
            .spells
            .iter()
            .filter(|(from, to, _)| (from * 2..to * 2).contains(&half))
            .flat_map(|(_, _, kind)| [kind, kind]);
        let sighted = sitting
            .sightings
            .iter()
            .filter(|(minute, _)| minute * 2 == half)
            .map(|(_, kind)| kind);
        for kind in sighted.chain(spelled) {
            {
                match *kind {
                    "drank" => drank += 1,
                    "ate" => ate += 1,
                    // Tiredness is the two together, each apart too (#8123).
                    "tired" => (tired, yawned) = (tired + 1, yawned + 1),
                    "stretch" => (tired, stretched) = (tired + 1, stretched + 1),
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
            r#"{{"tired_s_ago":0,"tired_seen":{tired},"drank_seen":{drank},"ate_seen":{ate},"desk_work_s_ago":0,"desk_work":"computer","desk_s":0,"celebrate_seen":{celebrated},"yawn_seen":{yawned},"yawn_s_ago":0,"stretch_seen":{stretched},"stretch_s_ago":0}}"#,
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
fn the_seen_reactions_ship_with_a_rest_a_short_wait_and_one_function_to_stop_them() {
    let skills = seen(&SEEN);
    for skill in &skills {
        let name = &skill.name;
        assert_eq!(skill.budget, None, "{name}: no group's hour or gap");
        assert!(skill.rest.is_none(), "{name}: no rest after lines ignored");
        assert_eq!(skill.most_a_day, None, "{name}: no day's most");
        // Seen tired has a trigger and a line for a yawn and for a
        // stretch (#8123); the others one.
        let triggers = if name == "seen-tired" { 2 } else { 1 };
        assert_eq!(skill.triggers.len(), triggers, "{name}: a trigger a line");
        for trigger in &skill.triggers {
            // One remark a meal, which ends after 30 minutes with none seen
            // (the owner, 01:5x on #7668); a drink rests 15 minutes (the owner,
            // 02:1x, the first builder's yes 02:14); tired and celebrating 20.
            let (cooldown, still) = match name.as_str() {
                "seen-eat" => (0, Some(30)),
                "seen-drink" => (15, None),
                _ => (20, None),
            };
            assert_eq!(
                trigger.cooldown_us,
                cooldown * 60_000_000,
                "{name}: the per-kind rest"
            );
            assert_eq!(
                trigger.rearm_line.as_ref().map(|line| line.for_us),
                still.map(|minutes| minutes * 60_000_000),
                "{name}: the meal or stretch"
            );
            assert_eq!(
                trigger.wait_us,
                3 * 60_000_000,
                "{name}: a kept rise, 3 minutes"
            );
        }
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

/// Nothing seen leaves the lamp through the reactions (#7668, the first
/// builder's condition): each may read its five topics, speak and turn,
/// and nothing else (no service, write, message, camera, history, search
/// or watch), and every call they make is a turn to the face or a line.
/// What leaves the lamp stays lamp-seen's own, as before the switch-on.
#[test]
fn nothing_the_reactions_see_leaves_the_lamp_through_them() {
    for skill in seen(&SEEN) {
        let name = &skill.name;
        assert_eq!(
            skill.permissions,
            Permissions {
                topics: [
                    "humans/doing_more",
                    "voice/state",
                    "voice/privacy",
                    "expression/state",
                    "humans/faces/tracked"
                ]
                .map(String::from)
                .to_vec(),
                speak: true,
                body: true,
                // How the file was written, not a permission.
                format_2: skill.permissions.format_2,
                ..Permissions::default()
            },
            "{name}"
        );
        assert!(
            skill.history.is_none()
                && skill.search.is_none()
                && skill.knowledge.is_none()
                && skill.watch.is_none()
                && skill.tool.is_none(),
            "{name}"
        );
    }
    let sightings = [(10, "drank"), (11, "ate"), (12, "tired"), (13, "celebrate")];
    let (said, called) = play_calls(seen(&SEEN), &Sitting::of(20, &sightings), true);
    assert_eq!(said.len(), 4, "{said:?}");
    assert_eq!(called.len(), 8, "{called:?}");
    for (_, name, call) in &called {
        assert!(
            matches!(call, Call::Say(_) | Call::Gaze { .. }),
            "{name}: {call:?}"
        );
    }
}

#[test]
fn a_kind_rests_twenty_minutes_from_its_line_and_another_kind_may_come_sooner() {
    // A drink at 10, then each other kind a minute apart: a line each. The
    // drink at 14 is within 15 minutes of the one at 10 and says nothing;
    // the one at 31 is past them.
    let sightings = [
        (10, "drank"),
        (11, "ate"),
        (12, "celebrate"),
        (13, "tired"),
        (14, "drank"),
        (31, "drank"),
    ];
    let said = play(seen(&SEEN), &Sitting::of(35, &sightings));
    assert_eq!(
        named(&said),
        [
            (10, "seen-drink"),
            (11, "seen-eat"),
            (12, "seen-celebrate"),
            (13, "seen-tired"),
            (31, "seen-drink")
        ]
    );
}

/// A yawn is told as a yawn and a stretch as a stretch, never one for the
/// other (#8123: on lamp-06c1 a yawn was told as "Big stretch"); each has
/// its own 20 minutes' rest, so a stretch may follow a yawn sooner.
#[test]
fn a_yawn_and_a_stretch_are_each_told_as_what_was_seen() {
    let sightings = [
        (10, "tired"),
        (12, "stretch"),
        (14, "tired"),
        (16, "stretch"),
    ];
    let (said, called) = play_calls(seen(&SEEN), &Sitting::of(20, &sightings), true);
    assert_eq!(named(&said), [(10, "seen-tired"), (12, "seen-tired")]);
    let notes: Vec<(u64, &str)> = called
        .iter()
        .filter_map(|(minute, _, call)| match call {
            Call::Say(note) => Some((*minute, note.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 2, "{notes:?}");
    let (yawn, stretch) = (notes[0].1, notes[1].1);
    assert!(
        yawn.contains("saw them yawn") && !yawn.contains("saw them stretch"),
        "{yawn}"
    );
    assert!(
        stretch.contains("saw them stretch") && !stretch.contains("saw them yawn"),
        "{stretch}"
    );
    assert!(!yawn.contains("yawn or stretch") && !stretch.contains("yawn or stretch"));
}

#[test]
fn half_an_hour_seated_with_water_often_has_two_lines() {
    // The switch-on's row, played (#7401, the first builder's 19:05): 30
    // minutes seated, water drunk six times. Two lines, the second past the
    // first's 15 minutes (the owner, 02:1x on #7668).
    let sightings = [
        (2, "drank"),
        (6, "drank"),
        (11, "drank"),
        (17, "drank"),
        (23, "drank"),
        (28, "drank"),
    ];
    let said = play(seen(&SEEN), &Sitting::of(30, &sightings));
    assert_eq!(named(&said), [(2, "seen-drink"), (17, "seen-drink")]);
}

/// The owner's call, 01:5x on #7668: "30m for meal, 5m for drink".
#[test]
fn a_meal_has_one_remark_and_eating_again_past_half_an_hour_is_a_new_one() {
    // Eating seen every 15 s from 5 to 45, a 40-minute meal: one remark, at
    // 5. Its last bite just before 45; eating again at 76, 31 minutes
    // after: a new meal, a second remark.
    let spells = [(5, 45, "ate")];
    let sightings = [(76, "ate")];
    let sitting = Sitting {
        spells: &spells,
        ..Sitting::of(80, &sightings)
    };
    assert_eq!(
        named(&play(seen(&SEEN), &sitting)),
        [(5, "seen-eat"), (76, "seen-eat")]
    );
    // At 74, under 30 minutes after the last bite: the same meal.
    let sightings = [(74, "ate")];
    let sitting = Sitting {
        spells: &spells,
        ..Sitting::of(80, &sightings)
    };
    assert_eq!(named(&play(seen(&SEEN), &sitting)), [(5, "seen-eat")]);
}

/// The owner, 02:1x on #7668 (the first builder's yes, 02:14): a drink
/// rests 15 minutes from its last line heard.
#[test]
fn sips_for_half_an_hour_have_three_lines_fifteen_minutes_apart() {
    // A sip each minute from 1 to 31 (the sitting's first word, at 0, is
    // its count's first reading, so the first sip is at 1): lines at 1, 16
    // and 31, three in 31 minutes, 15 minutes apart.
    let sips: Vec<(u64, &str)> = (1..=31).map(|at| (at, "drank")).collect();
    let said = play(seen(&SEEN), &Sitting::of(32, &sips));
    assert_eq!(
        named(&said),
        [(1, "seen-drink"), (16, "seen-drink"), (31, "seen-drink")]
    );
    // A sip every 2 minutes, 2 to 34: the first sip 15 or more minutes past
    // each line, at 2, 18 and 34.
    let sips: Vec<(u64, &str)> = (2..=34).step_by(2).map(|at| (at, "drank")).collect();
    let said = play(seen(&SEEN), &Sitting::of(35, &sips));
    assert_eq!(
        named(&said),
        [(2, "seen-drink"), (18, "seen-drink"), (34, "seen-drink")]
    );
}

#[test]
fn rearm_still_is_refused_but_alone_on_a_rise() {
    for (what, rearm) in [
        ("with for", "rearm = { still = \"5m\", for = \"1m\" }"),
        ("with below", "rearm = { still = \"5m\", below = 1 }"),
        ("under a minute", "rearm = { still = \"30s\" }"),
        ("over an hour", "rearm = { still = \"2h\" }"),
    ] {
        let refused = drink_with(|toml| toml.replace("rearm = \"leaves\"", rearm));
        assert!(refused.is_err(), "{what}");
    }
    let no_rise = drink_with(|toml| {
        toml.replace("rise = { by = 1, within = \"45s\" }\n", "")
            .replace("rearm = \"leaves\"", "rearm = { still = \"5m\" }")
    });
    assert!(
        no_rise
            .as_ref()
            .is_err_and(|why| why.contains("on a trigger with a rise")),
        "{no_rise:?}"
    );
    let still = drink_with(|toml| toml.replace("rearm = \"leaves\"", "rearm = { still = \"5m\" }"));
    assert!(still.is_ok(), "{still:?}");
}

#[test]
fn a_rise_while_the_lamp_talks_is_said_once_it_rests() {
    // Seen at 10 in a talk from 9 to 12: said at 12, though lamp-seen's
    // count stopped rising two minutes before. Two seen in one talk are
    // one line after it.
    for kind in ["drank", "ate", "tired", "celebrate"] {
        let sightings = [(10, kind)];
        let sitting = Sitting {
            talks: &[(9, 12)],
            ..Sitting::of(20, &sightings)
        };
        let said = play(seen(&SEEN), &sitting);
        assert_eq!(said.len(), 1, "{kind}: {said:?}");
        assert_eq!(said[0].0, 12, "{kind}: {said:?}");
    }
    let sightings = [(10, "drank"), (11, "drank")];
    let sitting = Sitting {
        talks: &[(9, 12)],
        ..Sitting::of(20, &sightings)
    };
    assert_eq!(
        named(&play(seen(&["seen-drink"]), &sitting)),
        [(12, "seen-drink")]
    );
}

#[test]
fn a_rise_kept_past_three_minutes_is_dropped_never_said_after_the_turn() {
    // The first builder's 00:14 on #7668: `wait` counts from the sighting,
    // so a remark is never more than 3 minutes stale, and never comes
    // during a turn. Seen at 10 in a talk from 9 to 14: kept 3 minutes,
    // dropped at 13, nothing at 14 or after. In a 30-minute talk, 9 to 39:
    // nothing at its end either. In a talk from 9 to 12: said at 12.
    for kind in ["drank", "ate", "tired", "celebrate"] {
        let sightings = [(10, kind)];
        for talk in [(9, 14), (9, 39)] {
            let sitting = Sitting {
                talks: &[talk],
                ..Sitting::of(45, &sightings)
            };
            assert_eq!(play(seen(&SEEN), &sitting), [], "{kind}: {talk:?}");
        }
        let short = Sitting {
            talks: &[(9, 12)],
            ..Sitting::of(20, &sightings)
        };
        let said = play(seen(&SEEN), &short);
        assert_eq!(said.len(), 1, "{kind}: {said:?}");
        assert_eq!(said[0].0, 12, "{kind}: {said:?}");
    }
}

/// Heron's P2 on #7668: the voice renews `voice/privacy` every 10 ms, so
/// lamp-skills never subscribes to it for the four; `main` tells the slide
/// on a change (`Engine::privacy`).
#[test]
fn the_reactions_add_no_subscription_to_the_privacy_lease() {
    let engine = Engine::new(seen(&SEEN), true);
    let topics = engine.topics();
    assert!(
        !topics.iter().any(|topic| topic == "voice/privacy"),
        "{topics:?}"
    );
    assert!(
        topics.iter().any(|topic| topic == "humans/doing_more"),
        "{topics:?}"
    );
    assert!(engine.reads_privacy());
}

/// The seen-drink folder copied with its `skill.toml` changed by `edit`,
/// loaded.
fn drink_with(edit: impl Fn(&str) -> String) -> Result<Skill, String> {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("seen-drink");
    std::fs::create_dir(&folder).unwrap();
    let from = skills_root().join("seen-drink");
    std::fs::copy(from.join("SKILL.md"), folder.join("SKILL.md")).unwrap();
    let toml = std::fs::read_to_string(from.join("skill.toml")).unwrap();
    assert!(toml.contains("drops_kept = true\n"));
    std::fs::write(folder.join("skill.toml"), edit(&toml)).unwrap();
    load(&folder).map_err(|error| error.to_string())
}

#[test]
fn drops_kept_is_refused_but_on_the_privacy_slides_plain_test() {
    for (what, edit) in [
        ("with for", "drops_kept = true\nfor = \"5s\"\n"),
        ("with unknown", "drops_kept = true\nunknown = \"holds\"\n"),
        ("not a bool", "drops_kept = \"yes\"\n"),
    ] {
        let refused = drink_with(|toml| toml.replace("drops_kept = true\n", edit));
        assert!(refused.is_err(), "{what}");
    }
    let other_topic = drink_with(|toml| {
        toml.replace(
            "field = \"asleep\"\nequals = false\n",
            "field = \"asleep\"\nequals = false\ndrops_kept = true\n",
        )
    });
    assert!(
        other_topic
            .as_ref()
            .is_err_and(|why| why.contains("drops_kept is for voice/privacy")),
        "{other_topic:?}"
    );
    assert!(drink_with(|toml| toml.to_owned()).is_ok());
}

#[test]
fn drops_kept_false_or_left_out_changes_nothing() {
    // Without it, the slide is an ordinary `[[while]]`: read as before
    // (subscribed), and a rise kept through a talk is kept through the
    // slide's closing too, said once both hold.
    for edit in ["drops_kept = false\n", ""] {
        let drink = drink_with(|toml| toml.replace("drops_kept = true\n", edit)).unwrap();
        let mut engine = Engine::new(vec![drink], true);
        assert!(!engine.reads_privacy(), "{edit:?}");
        assert!(engine.topics().iter().any(|topic| topic == "voice/privacy"));
        let _yes = seen_more_yes(&mut engine, &["seen-drink"]);
        let sightings = [(10, "drank")];
        let sitting = Sitting {
            talks: &[(9, 12)],
            ..Sitting::of(20, &sightings)
        };
        // The slide said as a message, as a subscribed topic is.
        let said = play_on(&mut engine, &sitting, |engine, at, _| {
            let minute = at / 60_000_000;
            let open = !(11..12).contains(&minute);
            let told = format!(r#"{{"permitted":{open}}}"#);
            engine.observe("voice/privacy", at, told.as_bytes(), None);
        });
        assert_eq!(named(&said), [(12, "seen-drink")], "{edit:?}");
    }
}

#[test]
fn closing_the_privacy_slide_drops_a_kept_rise_at_once() {
    // Seen at 10 in a talk from 9 to 12, within its 3 minutes: said at 12
    // with the slide open. Closed from 11 to 12, while it was kept, and
    // open again as the talk ends: nothing, then or after.
    for kind in ["drank", "ate", "tired", "celebrate"] {
        let sightings = [(10, kind)];
        let open = Sitting {
            talks: &[(9, 12)],
            ..Sitting::of(20, &sightings)
        };
        let said = play(seen(&SEEN), &open);
        assert_eq!(said.len(), 1, "{kind}: open: {said:?}");
        assert_eq!(said[0].0, 12, "{kind}: open: {said:?}");
        let closed = Sitting {
            closed: &[(11, 12)],
            ..open
        };
        assert_eq!(play(seen(&SEEN), &closed), [], "{kind}: closed");
    }
}

#[test]
fn nothing_is_said_with_the_slide_closed() {
    // The slide closed from 9 to 15, a drink at 10: nothing, and nothing
    // once it opens.
    let sightings = [(10, "drank")];
    let closed = Sitting {
        closed: &[(9, 15)],
        ..Sitting::of(30, &sightings)
    };
    assert_eq!(play(seen(&SEEN), &closed), []);
}

#[test]
fn a_remark_at_night_is_said_as_normal() {
    // No quiet hours (the owner, 20 Nov 13:1x on #7668: someone may work
    // late or wake early). A drink at 22:05 is said; a yawn at 07:50 and a
    // drink at 08:05 are both said.
    let sightings = [(15, "drank")];
    let night = Sitting {
        starts: 21 * 3600 + 50 * 60,
        ..Sitting::of(30, &sightings)
    };
    assert_eq!(named(&play(seen(&SEEN), &night)), [(15, "seen-drink")]);
    let sightings = [(10, "tired"), (25, "drank")];
    let morning = Sitting {
        starts: 7 * 3600 + 40 * 60,
        ..Sitting::of(30, &sightings)
    };
    assert_eq!(
        named(&play(seen(&SEEN), &morning)),
        [(10, "seen-tired"), (25, "seen-drink")]
    );
}

#[test]
fn a_line_never_heard_costs_nothing() {
    // The drink's lines at 10 and 12 are dropped unheard (the voice's
    // `no_audio_answer`: `main` gives the run back with `Engine::unsaid`);
    // eating's at 11 is heard. An unheard line begins no rest, so the drink
    // at 12 and 14 still has its line; and a "stop that" at 13 stops
    // eating, the one heard last, not the drink.
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
    // Heard, the drink at 10 rests its kind: the drinks at 12 and 14 say
    // nothing.
    let mut engine = Engine::new(seen(&SEEN), true);
    let _yes = seen_more_yes(&mut engine, &SEEN);
    let said = play_on(&mut engine, &Sitting::of(16, &sightings), |_, _, _| {});
    assert_eq!(named(&said), [(10, "seen-drink"), (11, "seen-eat")]);
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

/// [`play_on`] seen-drink alone, drinks seen at `sightings` in a talk from
/// 9 to `talk_end` minutes past the drink's `wait` from 10, `minutes` long
/// in all; what it said, each `chance dropped` line at its minute, and the
/// minute the wait from 10 runs out.
#[allow(clippy::type_complexity)]
fn drink_past_its_wait(
    sightings: impl Fn(u64) -> Vec<u64>,
    talk_end: u64,
    minutes: u64,
) -> (Vec<(u64, String)>, Vec<(u64, String)>, u64, u64) {
    let skills = seen(&["seen-drink"]);
    let wait_us = skills[0].triggers[0].wait_us;
    assert_eq!(wait_us % 60_000_000, 0, "a wait of whole minutes");
    let expires = 10 + wait_us / 60_000_000;
    let talks = [(9, expires + talk_end)];
    let sightings: Vec<(u64, &str)> = sightings(expires)
        .into_iter()
        .map(|minute| (minute, "drank"))
        .collect();
    let sitting = Sitting {
        talks: &talks,
        ..Sitting::of(expires + minutes, &sightings)
    };
    let mut engine = Engine::new(skills, true);
    let _yes = seen_more_yes(&mut engine, &["seen-drink"]);
    let mut drops = Vec::new();
    let said = play_on(&mut engine, &sitting, |_, at, runs| {
        for run in runs {
            for event in &run.events {
                if event.why.contains("chance dropped") {
                    drops.push((at / 60_000_000, event.why.clone()));
                }
            }
        }
    });
    (said, drops, expires, wait_us / 1_000_000)
}

#[test]
fn a_rise_kept_past_its_wait_says_its_chance_dropped_once_and_nothing_else() {
    // Seen at 10 in a talk that outlasts the drink's `wait`: the rise kept
    // for it is let go at the half-minute its wait runs out, and that says
    // its "chance dropped" line once; nothing is said, in the talk or after.
    let (said, drops, expires, wait_s) = drink_past_its_wait(|_| vec![10], 3, 6);
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(drops.len(), 1, "{drops:?}");
    assert_eq!(drops[0].0, expires, "{drops:?}");
    assert!(
        drops[0]
            .1
            .contains(&format!("did not hold within its wait of {wait_s} s")),
        "{drops:?}"
    );
}

#[test]
fn a_new_rise_after_a_kept_rise_dropped_past_its_wait_still_has_its_line() {
    // The line says the drop and takes nothing: a second drink in the same
    // sitting, after the talk, has its remark, as before the line.
    let (said, drops, expires, _) = drink_past_its_wait(|expires| vec![10, expires + 5], 3, 8);
    assert_eq!(named(&said), [(expires + 5, "seen-drink")]);
    assert_eq!(drops.len(), 1, "{drops:?}");
    assert_eq!(drops[0].0, expires, "{drops:?}");
}
