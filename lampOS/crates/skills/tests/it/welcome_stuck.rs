//! lamp-06c1, build 5627: after a welcome back whose say expired, later
//! arrivals gave no Triggered until lamp-skills restarted.
use lamp_msgs::SkillStage;
use lamp_skills::{Engine, load};
use std::{fs, path::Path};

const S: u64 = 1_000_000;

/// Welcome back as it ran on lamp-06c1: away 30 s, cooldown 1 m.
fn lamp_copy(dir: &Path) -> lamp_skills::Skill {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/welcome-back");
    let to = dir.join("welcome-back");
    fs::create_dir_all(to.join("lights")).unwrap();
    fs::copy(from.join("SKILL.md"), to.join("SKILL.md")).unwrap();
    fs::copy(
        from.join("lights/warm_glow.toml"),
        to.join("lights/warm_glow.toml"),
    )
    .unwrap();
    let toml = fs::read_to_string(from.join("skill.toml"))
        .unwrap()
        .replace("absent_for = \"5m\"", "absent_for = \"30s\"")
        .replace("cooldown = \"20m\"", "cooldown = \"1m\"")
        .replace("for = \"5s\"", "for = \"2s\"");
    fs::write(to.join("skill.toml"), toml).unwrap();
    load(&to).unwrap()
}

const FACE: &[u8] = br#"{"faces":[{"id":1,"xmin":0.4,"xmax":0.6,"ymin":0.3,"ymax":0.55}]}"#;
const NOBODY: &[u8] = br#"{"faces":[]}"#;

fn triggered(runs: &[lamp_skills::Run]) -> bool {
    runs.iter()
        .any(|run| run.events.iter().any(|e| e.stage == SkillStage::Triggered))
}

/// One second of the bus: the voice's state, then the eyes' faces if any.
fn second(engine: &mut Engine, at: u64, voice: &str, faces: Option<&[u8]>) -> bool {
    let mut runs = engine.tick(at * S);
    let state = format!("{{\"state\":\"{voice}\"}}");
    runs.extend(engine.observe("voice/state", at * S, state.as_bytes(), Some(3_000)));
    if let Some(faces) = faces {
        runs.extend(engine.observe("humans/faces/tracked", at * S, faces, Some(2_000)));
    }
    triggered(&runs)
}

#[test]
fn an_arrival_after_a_say_that_expired_is_greeted() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(vec![lamp_copy(dir.path())], true);
    let mut hellos = Vec::new();
    for at in 0..400 {
        let faces = match at {
            0..60 => Some(NOBODY),
            60..120 => Some(FACE),
            // The eyes stop: nothing on the topic for 40 s.
            120..160 => None,
            160..400 => Some(FACE),
            _ => None,
        };
        if second(&mut engine, at, "resting", faces) {
            hellos.push(at);
        }
    }
    assert_eq!(hellos, [63, 163]);
}

/// What lamp-06c1 showed: arrivals after the first gave no line at all.
/// A chance the `[[while]]` holds back past `wait` is let go in silence on
/// a trigger that is not daily, where a daily one says "chance dropped".
/// Here the voice never rests 2 s from 64 s on (as when the say expired),
/// so the arrival at 160 s is let go at 223 s: the journal must say so.
#[test]
fn a_chance_let_go_after_its_wait_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(vec![lamp_copy(dir.path())], true);
    let mut said = Vec::new();
    for at in 0..400u64 {
        let faces = match at {
            0..60 => Some(NOBODY),
            60..120 => Some(FACE),
            120..160 => None,
            _ => Some(FACE),
        };
        let voice = if at >= 64 && at % 4 < 2 {
            "listening"
        } else {
            "resting"
        };
        let mut runs = engine.tick(at * S);
        let state = format!("{{\"state\":\"{voice}\"}}");
        runs.extend(engine.observe("voice/state", at * S, state.as_bytes(), None));
        if let Some(faces) = faces {
            runs.extend(engine.observe("humans/faces/tracked", at * S, faces, Some(2_000)));
        }
        for run in runs {
            for event in run.events {
                said.push((at, event.stage, event.why));
            }
        }
    }
    let after_first: Vec<_> = said.iter().filter(|(at, _, _)| *at > 100).collect();
    assert!(
        after_first
            .iter()
            .any(|(_, stage, why)| *stage == SkillStage::Answered && why.contains("dropped")),
        "the arrival at 160 s was let go with no line: {said:?}"
    );
}
