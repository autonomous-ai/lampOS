use super::*;
use crate::{
    attention::{Answers, Context},
    decide::Answer,
    replay::{Logged, Replayed, line},
};
use std::collections::BTreeMap;

/// The seven answers a model that answers well gives for a line whose
/// truth is `call` (the lamp is spoken to), `business` (people's own talk)
/// or neither (a question in a quiet room), with `jitter` from 0 to 0.1.
fn truth(call: bool, business: bool, jitter: f64) -> [f64; 7] {
    let yes = |when: bool, high: f64, low: f64| if when { high - jitter } else { low + jitter };
    [
        yes(call, 0.9, 0.1),
        0.1 + jitter,
        yes(business, 0.9, 0.1),
        yes(!business, 0.8, 0.3),
        0.1,
        0.1 + jitter / 2.0,
        0.05,
    ]
}

/// `n` labelled lines as `lamp-policy-replay` prints them, drawn from
/// `seed`: a third call the lamp, in either room (to be answered); a third
/// are people's own talk in a busy room (left alone); and the rest are
/// questions in a quiet room (to be answered).
/// Each model's answers are `answers(truth)`, and its choice is made from
/// them by the day's weights, as the run makes it.
/// How a model answers, from what a model that answers well would.
type Answering = fn([f64; 7]) -> [f64; 7];

fn run(n: usize, seed: u64, models: &[(&str, Answering)]) -> Vec<Value> {
    let mut draw = Draw(seed);
    let names = terms();
    (0..n)
        .map(|index| {
            let kind = draw.next() % 3;
            let quiet = match kind {
                0 => draw.next().is_multiple_of(2),
                1 => false,
                _ => true,
            };
            let jitter = (draw.next() % 100) as f64 / 1_000.0;
            let (call, business) = (kind == 0, kind == 1);
            let should = if business {
                Should::StayQuiet
            } else {
                Should::Answer
            };
            let context = Context {
                heard: String::new(),
                room: if quiet { Room::Quiet } else { Room::Busy },
                lamp_is_speaking: false,
                lamp_last_said: None,
                seconds_since_lamp_spoke: None,
                name_heard: None,
                like_asker: None,
            };
            let replayed: Vec<(String, Replayed)> = models
                .iter()
                .map(|(model, answers)| {
                    let seven = answers(truth(call, business, jitter));
                    let asked: BTreeMap<String, Answer> = names
                        .iter()
                        .zip(seven)
                        .map(|(name, yes)| (name.clone(), Answer::Noul(yes as f32)))
                        .collect();
                    let judgement = attention::combine(&Answers::of(&asked).unwrap(), &context);
                    (
                        (*model).to_owned(),
                        Replayed::Chose {
                            by: (*model).to_owned(),
                            choice: judgement.choice.as_str().to_owned(),
                            probability: f64::from(judgement.probability),
                            took_ms: 400,
                            answers: names.iter().cloned().zip(seven).collect(),
                        },
                    )
                })
                .collect();
            let logged = Logged {
                id: format!("2026-10-15#{}", index + 1),
                at_unix_ms: 1_760_000_000_000 + index as u64 * 1_000,
                context,
                choice: "stay_quiet".into(),
                probability: 0.3,
                by: "typesafe/jev-1.13-20260917".into(),
                follows_lamp: None,
            };
            line(&logged, Some(should), &replayed)
        })
        .collect()
}

fn as_given(seven: [f64; 7]) -> [f64; 7] {
    seven
}

/// Another scale: the two questions that carry most weight answered the
/// other way round.
fn turned(mut seven: [f64; 7]) -> [f64; 7] {
    seven[0] = 1.0 - seven[0];
    seven[2] = 1.0 - seven[2];
    seven
}

fn share(counted: &Value) -> f64 {
    counted["right"].as_f64().unwrap() / counted["labelled"].as_f64().unwrap()
}

/// The case the refit is for: a model whose answers mean as much but are on
/// another scale does badly by the day's weights and as well as the other
/// once fitted to its own answers, counted on the same halves.
#[test]
fn a_model_on_another_scale_is_told_apart_from_a_worse_one() {
    let models = [("same", as_given as Answering), ("turned", turned)];
    let lines = run(240, 7, &models);
    let names: Vec<String> = models.iter().map(|(name, _)| (*name).to_owned()).collect();
    let counted = summary(&lines, &names);
    assert_eq!(counted["status"], "refit_summary");
    let same = &counted["models"]["same"];
    let turned = &counted["models"]["turned"];
    // Both counted on the same halves: 120 kept back each time.
    assert_eq!(same["day_weights"]["right"]["labelled"], 120.0);
    assert_eq!(turned["refitted"]["right"]["labelled"], 120.0);
    assert_eq!(
        same["refitted"]["right_busy"]["labelled"],
        turned["refitted"]["right_busy"]["labelled"]
    );
    assert!(share(&same["day_weights"]["right"]) > 0.9, "{same}");
    assert!(share(&same["refitted"]["right"]) > 0.9, "{same}");
    assert!(share(&turned["day_weights"]["right"]) < 0.7, "{turned}");
    assert!(share(&turned["refitted"]["right"]) > 0.9, "{turned}");
    // The weights on every label say which way each one answers.
    let weight =
        |model: &Value, term: &str| model["weights_on_every_label"][term].as_f64().unwrap();
    assert!(weight(same, "calls_lamp") > 0.0 && weight(same, "others_business") < 0.0);
    assert!(weight(turned, "calls_lamp") < 0.0 && weight(turned, "others_business") > 0.0);
    assert!(weight(same, "quiet") > 0.0);
    assert_eq!(same["halves_unfitted"], 0);
    assert_eq!(counted["words"], "none: counts and weights only");
}

/// Lines from a run before the answers were kept: nothing to fit, and said
/// so, not a fit of nothing.
#[test]
fn a_run_without_answers_is_not_fitted() {
    let mut lines = run(40, 3, &[("old", as_given as Answering)]);
    for line in &mut lines {
        line["judges"][0].as_object_mut().unwrap().remove("answers");
    }
    let counted = summary(&lines, &["old".to_owned()]);
    assert_eq!(
        counted["models"]["old"],
        json!({"labelled": 40, "without_answers": 40, "fitted": false,
               "why_not": "fewer than 20 labelled lines with answers"})
    );
    // Lines nobody labelled are not counted at all.
    let mut unlabelled = run(40, 3, &[("old", as_given as Answering)]);
    for line in &mut unlabelled {
        line["should"] = Value::Null;
    }
    let counted = summary(&unlabelled, &["old".to_owned()]);
    assert_eq!(counted["models"]["old"]["labelled"], 0);
}

/// The reviewer on #873: labels of one kind, on every line or on the lines
/// a model answered, give no fit, and the model is said not fitted, why,
/// and given no counts that could be taken for measured ones. A balanced
/// run is fitted on every half.
#[test]
fn a_model_with_labels_of_one_kind_is_not_fitted_and_given_no_counts() {
    let models = ["a".to_owned(), "b".to_owned()];
    let mut lines = run(
        90,
        3,
        &[("a", as_given as Answering), ("b", as_given as Answering)],
    );
    let counted = summary(&lines, &models);
    for model in &models {
        let told = &counted["models"][model];
        assert_eq!(told["fitted"], true, "{told}");
        assert_eq!(
            (&told["halves_fitted"], &told["halves_unfitted"]),
            (&json!(200), &json!(0))
        );
    }
    // `b` gave no answers on the lines to be answered: what it answered is
    // all to be left alone.
    let mut to_answer = 0;
    for line in &mut lines {
        if line["should"] == "answer" {
            to_answer += 1;
            line["judges"][1].as_object_mut().unwrap().remove("answers");
        }
    }
    let counted = summary(&lines, &models);
    assert_eq!(counted["models"]["a"]["fitted"], true);
    assert_eq!(
        counted["models"]["b"],
        json!({"labelled": 90, "without_answers": to_answer, "fitted": false,
               "why_not": "its labelled lines with answers are all of one kind"})
    );
    // Every line to be left alone: neither is fitted.
    for line in &mut lines {
        line["should"] = json!("stay_quiet");
    }
    let counted = summary(&lines, &models);
    for model in &models {
        let told = &counted["models"][model];
        assert_eq!(told["fitted"], false, "{told}");
        for count in [
            "day_weights",
            "refitted",
            "weights_on_every_label",
            "halves_fitted",
        ] {
            assert!(told.get(count).is_none(), "{count} in {told}");
        }
    }
}

/// Fitted on many examples drawn from known weights, the weights found are
/// those, near enough; and the order of the terms is the seven questions,
/// then the room, the conversation and the name.
#[test]
fn the_fit_finds_the_weights_the_examples_were_drawn_from() {
    assert_eq!(
        terms(),
        [
            "calls_lamp",
            "names_someone_else",
            "others_business",
            "lamp_could_answer",
            "follows_lamp",
            "listening_noise",
            "asks_to_stop",
            "quiet",
            "in_conversation",
            "name_heard",
        ]
    );
    let known: Weights = [-0.5, 2.0, -0.5, -2.0, 1.0, 0.0, -1.0, 0.5, 1.5, -0.3, 2.5];
    let mut draw = Draw(11);
    let mut unit = move || (draw.next() % 10_001) as f64 / 10_000.0;
    let examples: Vec<Example> = (0..4_000)
        .map(|place| {
            let mut terms = [0.0; TERMS];
            for (index, term) in terms.iter_mut().enumerate() {
                *term = if index < 7 {
                    unit()
                } else {
                    f64::from(u8::from(unit() < 0.5))
                };
            }
            let should = if unit() < probability(&known, &terms) {
                Should::Answer
            } else {
                Should::StayQuiet
            };
            Example {
                place,
                terms,
                should,
                chose: "answer".into(),
                named: false,
                busy: false,
                other_language: false,
            }
        })
        .collect();
    let all: Vec<&Example> = examples.iter().collect();
    let found = fit(&all, 1.0).unwrap();
    for (index, (found, known)) in found.iter().zip(known).enumerate() {
        assert!((found - known).abs() < 0.5, "{index}: {found} for {known}");
    }
    // All of one kind: no fit.
    let one_kind: Vec<&Example> = examples
        .iter()
        .filter(|example| example.should == Should::Answer)
        .collect();
    assert!(fit(&one_kind, 1.0).is_none());

    // Answering what was not for the lamp made to cost three times an
    // ignored request (#871): the sum starts lower, so fewer of those to be
    // left alone are taken up, and more of those to be answered are not.
    let costly = fit(&all, 3.0).unwrap();
    assert!(
        costly[0] < found[0] - 0.5,
        "{} against {}",
        costly[0],
        found[0]
    );
    let taken_up = |weights: &Weights, should: Should| {
        examples
            .iter()
            .filter(|example| example.should == should)
            .filter(|example| {
                crate::attention::Choice::at(probability(weights, &example.terms) as f32)
                    != crate::attention::Choice::StayQuiet
            })
            .count()
    };
    assert!(taken_up(&costly, Should::StayQuiet) < taken_up(&found, Should::StayQuiet));
    assert!(taken_up(&costly, Should::Answer) <= taken_up(&found, Should::Answer));
}

/// With a cost, the summary says it, and what the mistakes cost on the
/// kept-back halves, for the day's weights and the refit alike.
#[test]
fn the_cost_of_the_mistakes_is_counted_with_the_ratio_given() {
    let lines = run(240, 7, &[("same", as_given as Answering)]);
    let models = ["same".to_owned()];
    let even = summary(&lines, &models);
    assert_eq!(even["cost_ratio"], 1.0);
    let told = &even["models"]["same"]["day_weights"];
    let cost = |told: &Value| told["cost"].as_f64().unwrap();
    let mistakes = |told: &Value| {
        told["answered_not_for_the_lamp"].as_f64().unwrap()
            + told["ignored_for_the_lamp"].as_f64().unwrap()
    };
    assert!((cost(told) - mistakes(told)).abs() < 0.11, "{told}");
    let costly = summary_costing(&lines, &models, 3.0);
    assert_eq!(costly["cost_ratio"], 3.0);
    let told = &costly["models"]["same"]["day_weights"];
    let three = 3.0 * told["answered_not_for_the_lamp"].as_f64().unwrap()
        + told["ignored_for_the_lamp"].as_f64().unwrap();
    assert!((cost(told) - three).abs() < 0.11, "{told}");
}

/// The halves are drawn from the seed alone: two counts of one run are the
/// same, on any machine.
#[test]
fn the_halves_are_the_same_every_time() {
    assert_eq!(Draw(SEED).order(8), Draw(SEED).order(8));
    let mut sorted = Draw(SEED).order(50);
    sorted.sort_unstable();
    assert_eq!(sorted, (0..50).collect::<Vec<_>>());
    let lines = run(60, 5, &[("same", as_given as Answering)]);
    let models = ["same".to_owned()];
    assert_eq!(summary(&lines, &models), summary(&lines, &models));
}
