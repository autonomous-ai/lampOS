//! Authored numerical fixtures, not recorded speech or a lamp benchmark.
use lamp_policy::{
    attention::{self, Answers, Choice},
    decide::{Answer, Decider, Failed, Question},
    policy::local::{
        Cues, Local, MOST_CUES_BYTES, MOST_WEIGHTS_BYTES, QUESTION_NAMES, RoomCue, Weights,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const DEADLINE: Duration = Duration::from_secs(1);

fn judge(local: &mut Local, cues: Cues) -> f32 {
    let asked = local
        .ask(
            &serde_json::to_string(&cues).unwrap(),
            &attention::questions(),
            DEADLINE,
        )
        .unwrap();
    local
        .weights()
        .judge(&Answers::of(&asked.answers).unwrap())
        .probability
}

struct Place(PathBuf);
impl Place {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lamp-local-model-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn load(&self, value: &Value) -> std::io::Result<Weights> {
        let path = self.0.join("weights.json");
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        Weights::load(&path)
    }
}
impl Drop for Place {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn all_unknown_is_exactly_the_choice_prior_and_not_jev_calibration() {
    let weights = Weights::seed();
    let data = serde_json::to_value(&weights).unwrap();
    let bias = data["local_choice"]["bias"].as_f64().unwrap();
    let expected = (1.0 / (1.0 + (-bias).exp())) as f32;
    let mut local = Local::new(weights);
    assert_eq!(judge(&mut local, Cues::default()), expected);
    let asked = local.ask("{}", &attention::questions(), DEADLINE).unwrap();
    assert_eq!(asked.answers.len(), 7);
    assert!(
        QUESTION_NAMES.iter().all(
            |name| matches!(asked.answers[*name], Answer::Noul(p) if (0.0..=1.0).contains(&p))
        )
    );
    let judgement = local.weights().judge(&Answers::of(&asked.answers).unwrap());
    assert_eq!(judgement.choice, Choice::LookUp);
    assert!(!judgement.from_rule);
    assert!(asked.usage.is_none() && asked.connection.is_none());
    assert_eq!(asked.model, "lamp-local-seed-v1-unfitted");
    assert_eq!(local.name(), asked.model);
    assert_eq!(local.weights().local_share(), 0.5);
}

#[test]
fn each_known_cue_alone_moves_the_seed_in_the_declared_direction() {
    let mut local = Local::new(Weights::seed());
    let neutral = judge(&mut local, Cues::default());
    let up = [
        Cues {
            name_score: Some(1.0),
            ..Cues::default()
        },
        Cues {
            conversation: Some(true),
            ..Cues::default()
        },
        Cues {
            room: Some(RoomCue::Quiet),
            ..Cues::default()
        },
        Cues {
            facing: Some(true),
            face_age_ms: Some(0),
            ..Cues::default()
        },
        Cues {
            since_lamp_spoke_ms: Some(0),
            ..Cues::default()
        },
        Cues {
            word_count: Some(12),
            ..Cues::default()
        },
        Cues {
            duration_ms: Some(6000),
            ..Cues::default()
        },
        Cues {
            text_called: Some(true),
            ..Cues::default()
        },
    ];
    for cues in up {
        assert!(judge(&mut local, cues) > neutral);
    }
    let down = [
        Cues {
            name_score: Some(0.0),
            ..Cues::default()
        },
        Cues {
            conversation: Some(false),
            ..Cues::default()
        },
        Cues {
            room: Some(RoomCue::Busy),
            ..Cues::default()
        },
        Cues {
            facing: Some(false),
            face_age_ms: Some(0),
            ..Cues::default()
        },
        Cues {
            since_lamp_spoke_ms: Some(40_000),
            ..Cues::default()
        },
        Cues {
            word_count: Some(1),
            ..Cues::default()
        },
        Cues {
            duration_ms: Some(100),
            ..Cues::default()
        },
    ];
    for cues in down {
        assert!(judge(&mut local, cues) < neutral);
    }
}

#[test]
fn unknown_pose_age_old_faces_and_negative_text_call_are_neutral() {
    let mut local = Local::new(Weights::seed());
    let neutral = judge(&mut local, Cues::default());
    for cues in [
        Cues {
            facing: Some(false),
            ..Cues::default()
        },
        Cues {
            facing: Some(true),
            ..Cues::default()
        },
        Cues {
            face_age_ms: Some(0),
            ..Cues::default()
        },
        Cues {
            facing: Some(true),
            face_age_ms: Some(2001),
            ..Cues::default()
        },
        Cues {
            facing: Some(false),
            face_age_ms: Some(u32::MAX),
            ..Cues::default()
        },
        Cues {
            text_called: Some(false),
            ..Cues::default()
        },
    ] {
        assert_eq!(judge(&mut local, cues), neutral);
    }
    let old = judge(
        &mut local,
        Cues {
            facing: Some(true),
            face_age_ms: Some(1000),
            ..Cues::default()
        },
    );
    let fresh = judge(
        &mut local,
        Cues {
            facing: Some(true),
            face_age_ms: Some(0),
            ..Cues::default()
        },
    );
    assert!(neutral < old && old < fresh);
}

#[test]
fn zero_and_expired_deadlines_fail_without_partial_answers() {
    let mut local = Local::new(Weights::seed());
    assert_eq!(
        local.ask("{}", &attention::questions(), Duration::ZERO),
        Err(Failed::TimedOut)
    );
    assert_eq!(
        local.ask("{}", &attention::questions(), Duration::from_nanos(1)),
        Err(Failed::TimedOut)
    );
    local.ready(Duration::ZERO);
    assert!(local.ask("{}", &attention::questions(), DEADLINE).is_ok());
}

#[test]
fn only_the_exact_seven_typed_questions_are_accepted() {
    let mut local = Local::new(Weights::seed());
    let mut questions = attention::questions();
    questions.pop();
    assert!(local.ask("{}", &questions, DEADLINE).is_err());
    let mut questions = attention::questions();
    questions[0].1 = Question::Choice {
        instructions: "different".into(),
        criteria: vec![],
    };
    assert!(local.ask("{}", &questions, DEADLINE).is_err());
    let mut questions = attention::questions();
    questions.swap(0, 1);
    assert!(local.ask("{}", &questions, DEADLINE).is_err());
    let mut questions = attention::questions();
    questions[0].1 = Question::Noul {
        instructions: "same name, different meaning".into(),
    };
    assert!(local.ask("{}", &questions, DEADLINE).is_err());
}

#[test]
fn malformed_and_oversized_cues_fail_without_echoing_private_content() {
    let mut local = Local::new(Weights::seed());
    for state in [
        "{secret_private_text",
        "{\"schema\":2}",
        "{\"name_score\":1.1}",
        "{\"name_score\":-0.1}",
        "{\"name_score\":1e999}",
        "{\"word_count\":-1}",
        "{\"heard\":\"secret_private_text\"}",
        "{\"room\":\"unknown\"}",
        "{\"facing\":\"false\"}",
    ] {
        let error = local
            .ask(state, &attention::questions(), DEADLINE)
            .unwrap_err();
        assert!(!error.to_string().contains("secret_private_text"));
    }
    assert!(
        local
            .ask(
                &" ".repeat(MOST_CUES_BYTES + 1),
                &attention::questions(),
                DEADLINE
            )
            .is_err()
    );
    let invalid = Cues {
        name_score: Some(f32::NAN),
        ..Cues::default()
    };
    assert!(invalid.validate().is_err());
    let cues = Cues {
        word_count: Some(u32::MAX),
        duration_ms: Some(u32::MAX),
        since_lamp_spoke_ms: Some(u32::MAX),
        ..Cues::default()
    };
    assert!(judge(&mut local, cues).is_finite());
}

#[test]
fn model_loading_checks_shape_names_coefficients_provenance_and_status() {
    let place = Place::new();
    let seed = serde_json::to_value(Weights::seed()).unwrap();
    assert_eq!(
        serde_json::to_value(place.load(&seed).unwrap()).unwrap(),
        seed
    );
    let mutations = [
        ("/schema", json!(2)),
        ("/feature_schema", json!(2)),
        ("/local_share", json!(1.1)),
        ("/name", json!("")),
        ("/questions/0/name", json!("something_else")),
        ("/questions/0/head/weights", json!([0, 0])),
        ("/questions/0/head/bias", json!(33)),
        ("/questions/0/head/state", json!("fitted")),
        ("/questions/0/head/state", json!("prior_only")),
        ("/provenance/rights", json!([])),
        ("/provenance/rows", json!(10001)),
    ];
    for (pointer, value) in mutations {
        let mut model = seed.clone();
        *model.pointer_mut(pointer).unwrap() = value;
        assert!(place.load(&model).is_err(), "accepted {pointer}");
    }
    let mut extra = seed.clone();
    extra["unknown"] = json!(true);
    assert!(place.load(&extra).is_err());
    let path = place.0.join("too-large");
    fs::write(&path, vec![b' '; MOST_WEIGHTS_BYTES + 1]).unwrap();
    assert!(Weights::load(&path).is_err());
}
