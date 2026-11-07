//! Quiet focus and Rest your eyes in one engine (#5620, the PM's call (a)):
//! both are due when someone looks up after 45 minutes head down, and on a
//! lamp that installed Quiet focus its word stands in for Rest your eyes'.
//! A lamp without it, or a sitting with no focus, is as before. Replayed
//! from Quiet focus's own case, through the real loader and engine.
use lamp_skills::{Call, Engine, Recorded, load_with, read_recordings};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 1 January 2026, 10:00 UTC: the case's `clock = "10:00"`, as
/// `lamp-skills test` replays it.
const TEN_O_CLOCK_MS: u64 = 1_767_225_600_000 + 10 * 3_600_000;

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

fn case() -> Vec<Recorded> {
    read_recordings(
        &skills_root()
            .join("quiet-focus")
            .join("tests")
            .join("a-word-when-they-look-up"),
    )
    .unwrap()
}

/// Each say made, by skill and the second it was made from the case's
/// first message, with `skills` loaded (each turned on, as a case is).
fn says(skills: &[&str], messages: &[Recorded]) -> Vec<(String, u64)> {
    let skills = skills
        .iter()
        .map(|name| {
            let mut skill = load_with(&skills_root().join(name), false).unwrap();
            skill.enabled = true;
            skill
        })
        .collect();
    let mut engine = Engine::new(skills, true);
    let first_us = messages.first().map_or(0, |message| message.header.at_us);
    engine.clock(0, first_us, TEN_O_CLOCK_MS);
    let mut said = Vec::new();
    let mut keep = |runs: Vec<lamp_skills::Run>, at_us: u64| {
        for run in runs {
            if run.calls.iter().any(|call| matches!(call, Call::Say(_))) {
                said.push((run.skill, (at_us - first_us) / 1_000_000));
            }
        }
    };
    for message in messages {
        let at_us = message.header.at_us;
        engine.clock(0, at_us, TEN_O_CLOCK_MS + (at_us - first_us) / 1_000);
        keep(engine.tick(at_us), at_us);
        keep(
            engine.observe(
                &message.header.topic,
                at_us,
                &message.payload,
                message.header.expires_ms,
            ),
            at_us,
        );
    }
    said
}

/// The same case with the eyes saying `at_desk` all along: never head down,
/// so no focus comes due.
fn never_head_down() -> Vec<Recorded> {
    case()
        .into_iter()
        .map(|mut message| {
            if message.header.topic == "room/activity" {
                let mut value: Value = serde_json::from_slice(&message.payload).unwrap();
                if value["state"] == "head_down" {
                    value["state"] = "at_desk".into();
                }
                message.payload = serde_json::to_vec(&value).unwrap();
            }
            message
        })
        .collect()
}

/// Head down from 3 minutes, the focus due at 48, a look up at 50: with both
/// installed, Quiet focus's word then, and no Rest your eyes line in the
/// sitting (lamp-9adf, 7 Nov: Rest your eyes spoke 5 s into the look up and
/// took the wellbeing group's gap, so the word was refused).
#[test]
fn quiet_focus_word_stands_in_for_rest_your_eyes() {
    let said = says(&["quiet-focus", "rest-your-eyes"], &case());
    assert_eq!(
        said.iter()
            .map(|(skill, _)| skill.as_str())
            .collect::<Vec<_>>(),
        ["quiet-focus"],
        "{said:?}"
    );
    assert!((50 * 60..51 * 60).contains(&said[0].1), "{said:?}");
}

/// A product lamp, without Quiet focus: Rest your eyes says its line when
/// they look up, as today.
#[test]
fn rest_your_eyes_alone_still_speaks_in_that_sitting() {
    let said = says(&["rest-your-eyes"], &case());
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].0, "rest-your-eyes");
    assert!((50 * 60..51 * 60).contains(&said[0].1), "{said:?}");
}

/// Quiet focus installed, but never head down: no focus is pending, and
/// Rest your eyes says its line at 45 minutes, as it would without it.
#[test]
fn with_quiet_focus_and_no_focus_rest_your_eyes_still_speaks() {
    let said = says(&["quiet-focus", "rest-your-eyes"], &never_head_down());
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].0, "rest-your-eyes");
    assert!((45 * 60..46 * 60).contains(&said[0].1), "{said:?}");
}
