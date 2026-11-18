//! Replays saved lamp journals through the voice's decision code, in CI and
//! without a lamp (#7772). Each case in `tests/replay/*.json` is one line a
//! row kit saw on a lamp: its word count and timing, the signs the worker
//! read for it, and the eyes' looks through it as the kit sampled them
//! (`S` lines). No words and no names: a line is `words` placeholder words.
//!
//! The case is fed the way `second_provider::worker::Heeding::not_said_to_me`
//! feeds it: the looks into a [`SpeakingFaces`], [`Line::with_eyes`] over
//! the line's window, then [`judge`] inside [`awaiting_yes`]. The verdict is
//! `answered`, or the journal's `held: <reason>` ([`Held::reason`]). A case
//! with `pending: "#N"` is what an open PR changes: it must still differ
//! today, and that PR drops `pending` in its own diff. Format and limits:
//! `lampOS/docs/voice-test-fixtures.md`, "Replaying saved lamp journals".

use crate::said_to_me::{Line, Said, awaiting_yes, judge};
use crate::speaking_faces::SpeakingFaces;
use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
use std::time::{Duration, Instant};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    case: String,
    /// Where it came from on the Mac: the kit's out dir, journal, request.
    #[allow(dead_code)]
    source: String,
    /// The line's start and end, in seconds on the eyes sample's clock.
    line_s: [f32; 2],
    words: usize,
    #[serde(default)]
    question: bool,
    /// The lamp's name heard in its sound.
    #[serde(default)]
    named: bool,
    #[serde(default)]
    going_on: bool,
    /// A face turned to the lamp (the worker's `signs.turned`).
    #[serde(default)]
    turned: bool,
    /// The lamp waits for a yes or no of its own asking (`a_yes_held`).
    #[serde(default)]
    yes_awaited: bool,
    /// The lamp's own question waits for its answer (the worker's
    /// `signs.lamp_asked`, #7698).
    #[serde(default)]
    lamp_asked: bool,
    /// Another voice in the conversation (the worker's `another_voice`,
    /// #7726): the lone face's pose then gives none.
    #[serde(default)]
    another_voice: bool,
    /// The kit's eyes sample lines through the line:
    /// `S <s> <faces> <area> <yaw deg> <pitch deg> <facing> <looking> <speaking> <voice state>`.
    #[serde(default)]
    eyes: Vec<String>,
    expect: String,
    /// What the lamp journaled, when the kit's sample is not what lamp-voice
    /// saw (it samples the largest face every 0.25 s on its own).
    #[serde(default)]
    #[allow(dead_code)]
    journal: Option<String>,
    #[serde(default)]
    pending: Option<String>,
}

/// One `S` line as the eyes' frame: the largest face only, or none.
fn faces(sample: &str) -> (f32, Faces) {
    let column: Vec<&str> = sample.split_whitespace().collect();
    assert!(
        column.len() >= 9 && column[0] == "S",
        "an eyes sample line: {sample}"
    );
    let at = column[1].parse().expect("seconds");
    let yes_no = |value: &str| match value {
        "y" => Some(true),
        "n" => Some(false),
        _ => None,
    };
    let state = match column[8] {
        "yes" => Some(SpeakingState::Yes),
        "no" => Some(SpeakingState::No),
        "unknown" => Some(SpeakingState::Unknown),
        _ => None,
    };
    let face = Face {
        id: "f1".to_owned(),
        xmin: 0.3,
        ymin: 0.2,
        xmax: 0.6,
        ymax: 0.6,
        confidence: 1.0,
        yaw: column[4].parse::<f32>().ok().map(f32::to_radians),
        facing: yes_no(column[6]),
        pitch: column[5].parse::<f32>().ok().map(f32::to_radians),
        looking: yes_no(column[7]),
        speaking: state.map(|state| Speaking {
            state,
            score: 0.0,
            from: None,
        }),
        follows: None,
    };
    let none = column[2] == "0";
    (
        at,
        Faces {
            faces: if none { Vec::new() } else { vec![face] },
        },
    )
}

/// What the decision code says of one case.
fn verdict(case: &Case) -> String {
    let eyes = SpeakingFaces::default();
    // A line may begin before the kit's sample did: the clock starts a
    // minute before the sample's 0.
    let begin = Instant::now();
    let at = |seconds: f32| begin + Duration::from_secs_f32((seconds + 60.0).max(0.0));
    for (seq, sample) in case.eyes.iter().enumerate() {
        let (seconds, frame) = faces(sample);
        eyes.look(at(seconds), Some((("replay", seq as u64), &frame)));
    }
    let heard = vec!["word"; case.words].join(" ") + if case.question { "?" } else { "" };
    let (line, _, _) = Line {
        heard: &heard,
        name_heard: case.named,
        going_on: case.going_on,
        lamp_asked: case.lamp_asked || crate::services::answer_awaited(&heard),
        turned: case.turned,
        ..Line::default()
    }
    .with_eyes(
        &eyes,
        at(case.line_s[0]),
        at(case.line_s[1]),
        case.another_voice,
    );
    match awaiting_yes(case.yes_awaited, || judge(line)) {
        Said::Held(held) => held.reason().to_owned(),
        Said::Stands | Said::InConversation | Said::PoseUnread | Said::ToTheLamp => {
            "answered".to_owned()
        }
    }
}

#[test]
fn saved_lamp_journals_replay_to_their_verdicts() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/replay");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/replay")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    let mut failed = Vec::new();
    let mut ran = 0;
    for file in &files {
        let text = std::fs::read_to_string(file).expect("a fixture");
        let cases: Vec<Case> = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        for case in &cases {
            ran += 1;
            let got = verdict(case);
            let ok = match &case.pending {
                None => got == case.expect,
                Some(_) => got != case.expect,
            };
            if !ok {
                failed.push(format!(
                    "{} {}: got `{got}`, expect `{}`{}",
                    file.file_name().unwrap_or_default().to_string_lossy(),
                    case.case,
                    case.expect,
                    case.pending
                        .as_ref()
                        .map(|pr| format!(" (pending {pr}: it already passes, drop `pending`)"))
                        .unwrap_or_default()
                ));
            }
        }
    }
    assert!(ran > 0, "no cases in {}", dir.display());
    assert!(
        failed.is_empty(),
        "{} of {ran}:\n{}",
        failed.len(),
        failed.join("\n")
    );
}
