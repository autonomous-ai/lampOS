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

/// Exercise the real drop event, with no process, bus or wall-clock wait.
/// This fixture owns its files and never edits a product skill's wording.
fn dropped_condition_note(topic: &str, condition: &str, payload: serde_json::Value) -> String {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("drop-mask");
    fs::create_dir(&folder).unwrap();
    fs::write(
        folder.join("SKILL.md"),
        "---\nname: drop-mask\ndescription: A blocked test chance.\n---\n",
    )
    .unwrap();
    fs::write(
        folder.join("skill.toml"),
        format!(
            r#"
[permissions]
topics = ["air/state", "voice/state", "humans/faces/tracked"]
light = true

[trigger]
name = "blocked"
topic = "air/state"
field = "co2_ppm"
above = 1200
rearm = "ends"
wait = "2s"

[[while]]
topic = "{topic}"
{condition}

[ask]
policy = "nudge"

[[do]]
light = {{ dim = 1 }}
"#
        ),
    )
    .unwrap();
    let mut engine = Engine::new(vec![load(&folder).unwrap()], true);
    assert!(
        engine
            .observe(
                topic,
                0,
                &serde_json::to_vec(&payload).unwrap(),
                Some(10_000)
            )
            .is_empty()
    );
    let mut note = None;
    // The first active message starts the wait. The third expires it;
    // another active message must not spend or report the same chance again.
    // Both topics stay fresh throughout, including the face-count case.
    for at in 1..=4 {
        let runs = engine.observe("air/state", at * S, br#"{"co2_ppm":1300}"#, Some(10_000));
        if at != 3 {
            assert!(runs.is_empty(), "unexpected run at {at} s");
            continue;
        }
        assert_eq!(runs.len(), 1, "exactly one chance expires at {at} s");
        let run = &runs[0];
        assert_eq!(run.skill, "drop-mask");
        assert!(run.calls.is_empty(), "a blocked chance takes no action");
        assert!(run.ask.is_none());
        assert!(!run.queue);
        assert_eq!(run.events.len(), 1);
        let event = &run.events[0];
        assert_eq!(event.stage, SkillStage::Answered);
        assert_eq!(event.allowed, Some(false));
        note = Some(event.why.clone());
    }
    note.expect("one drop note at the end of the wait")
}

fn assert_masked_note(note: &str, held: &str, hidden: &[&str]) {
    assert!(note.starts_with("blocked chance dropped:"), "{note}");
    assert!(note.ends_with(&format!(" (held by {held})")), "{note}");
    assert_eq!(note.matches(" (held by ").count(), 1, "{note}");
    for value in hidden {
        assert!(!note.contains(value), "fixture contents leaked: {note}");
    }
}

fn assert_masked_value(value: serde_json::Value, shown: &str, hidden: &[&str]) {
    let note = dropped_condition_note(
        "voice/state",
        "field = \"state\"\nequals = \"resting\"",
        serde_json::json!({ "state": value }),
    );
    assert_masked_note(&note, &format!("voice/state state was {shown}"), hidden);
}

#[test]
fn drop_mask_hides_arbitrary_text() {
    // Unexpected values are still JSON the Engine observes. None may put
    // arbitrary words into a drop note, regardless of length or alphabet.
    for text in [
        "private fixture sentence",
        "private_fixture_word_longer_than_32_bytes",
        "fixture-é",
    ] {
        assert_masked_value(serde_json::json!(text), "a text", &[text]);
    }
    assert_masked_value(serde_json::json!(""), "a text", &[]);
}

#[test]
fn drop_mask_hides_list_contents() {
    assert_masked_value(
        serde_json::json!(["list-fixture-secret", { "list-object-secret": 91 }]),
        "a list of 2",
        &["list-fixture-secret", "list-object-secret"],
    );
}

#[test]
fn drop_mask_hides_object_contents() {
    assert_masked_value(
        serde_json::json!({ "object-fixture-key": "object-fixture-value" }),
        "an object",
        &["object-fixture-key", "object-fixture-value"],
    );
}

#[test]
fn drop_mask_counts_faces_without_their_contents() {
    let payload = serde_json::json!({ "faces": [
        { "id": "face-fixture-a", "xmin": 0.12345, "xmax": 0.4, "ymin": 0.2, "ymax": 0.6, "confidence": 0.9 },
        { "id": "face-fixture-b", "xmin": 0.6, "xmax": 0.9, "ymin": 0.23456, "ymax": 0.7, "confidence": 0.9 }
    ] });
    let faces: lamp_msgs::Faces = serde_json::from_value(payload.clone()).unwrap();
    faces.check().unwrap();
    assert_eq!(faces.faces.len(), 2);
    // The faces are present, but not for the five seconds this condition needs.
    let note = dropped_condition_note(
        "humans/faces/tracked",
        "present = true\nfor = \"5s\"",
        payload,
    );
    assert_masked_note(
        &note,
        "humans/faces/tracked faces was 2 faces for 3 s of its 5 s",
        &["face-fixture-a", "face-fixture-b", "0.12345", "0.23456"],
    );
}

#[test]
fn drop_mask_keeps_safe_scalar_descriptions() {
    for (value, shown) in [
        (serde_json::Value::Null, "nothing"),
        (serde_json::json!(true), "true"),
        (serde_json::json!(false), "false"),
        (serde_json::json!(17.25), "17.25"),
        (serde_json::json!("listening"), "listening"),
        (serde_json::json!("idle_state-2"), "idle_state-2"),
    ] {
        assert_masked_value(value, shown, &[]);
    }
}
